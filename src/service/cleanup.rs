//! 旧服务 / 孤儿进程 / 旧目录清理与迁移。
#![allow(missing_docs)]

use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use service_manager::{
    ServiceLabel, ServiceStatus, ServiceStatusCtx, ServiceStopCtx, ServiceUninstallCtx,
};
use tracing::{info, warn};

use super::health::{default_data_root, ListenerInfo, ServiceHealthReport};
use super::manage::native_manager;
use super::recovery::{begin_phase, clear_state, resume_if_incomplete, MigrationPhase};
use super::{
    LEGACY_SERVICE_QUALIFIED_NAME, SERVICE_GENERATION, SERVICE_QUALIFIED_NAME,
};

const DEFAULT_LISTEN: &str = "127.0.0.1:50051";

/// 修复选项。
#[derive(Debug, Clone, Default)]
pub struct RepairOptions {
    pub user: bool,
    pub migrate_legacy_data: bool,
}

/// 修复结果摘要。
#[derive(Debug, Clone, Default)]
pub struct RepairReport {
    pub stopped_current_service: bool,
    pub uninstalled_legacy_service: bool,
    pub killed_stale_listeners: u32,
    pub migrated_legacy_data: bool,
    pub removed_legacy_data_dir: bool,
    pub normalized_registry: bool,
}

fn default_listen() -> SocketAddr {
    DEFAULT_LISTEN.parse().expect("valid listen")
}

fn legacy_data_dir(root: &Path) -> PathBuf {
    root.join("instances").join("default")
}

fn is_legacy_command_line(cmd: &str) -> bool {
    let lower = cmd.to_lowercase();
    lower.contains("core-default")
        || lower.contains(r"instances\default")
        || lower.contains("instances/default")
}

fn generation_matches(cmd: &str) -> bool {
    cmd.contains(SERVICE_GENERATION)
}

pub fn find_listener(port: u16) -> Result<Option<ListenerInfo>> {
    #[cfg(windows)]
    {
        return find_listener_windows(port);
    }
    #[cfg(unix)]
    {
        return find_listener_unix(port);
    }
}

#[cfg(windows)]
fn find_listener_windows(port: u16) -> Result<Option<ListenerInfo>> {
    let script = format!(
        r#"
$conn = Get-NetTCPConnection -LocalPort {port} -State Listen -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $conn) {{ exit 1 }}
$pid = $conn.OwningProcess
$proc = Get-CimInstance Win32_Process -Filter "ProcessId=$pid" -ErrorAction SilentlyContinue
Write-Output "$pid"
if ($proc) {{ Write-Output $proc.CommandLine }}
"#
    );
    let out = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()?;
    if out.status.code() == Some(1) {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines().map(str::trim).filter(|s| !s.is_empty());
    let pid = lines
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    if pid == 0 {
        return Ok(None);
    }
    let cmd = lines.next().map(str::to_string);
    let cmd_text = cmd.clone().unwrap_or_default();
    Ok(Some(ListenerInfo {
        pid,
        command_line: cmd,
        is_astral_core: cmd_text.to_lowercase().contains("astral-core"),
        is_legacy: is_legacy_command_line(&cmd_text),
        generation_match: generation_matches(&cmd_text),
    }))
}

#[cfg(unix)]
fn find_listener_unix(port: u16) -> Result<Option<ListenerInfo>> {
    let lsof = Command::new("lsof")
        .args(["-t", "-iTCP", &format!(":{port}"), "-sTCP:LISTEN"])
        .output();
    let Ok(lsof) = lsof else {
        return Ok(None);
    };
    if !lsof.status.success() {
        return Ok(None);
    }
    let pid = lsof
        .stdout
        .split(|c| c.is_ascii_whitespace())
        .filter_map(|s| std::str::from_utf8(s).ok())
        .find_map(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    if pid == 0 {
        return Ok(None);
    }
    let ps = Command::new("ps").args(["-o", "command=", "-p", &pid.to_string()]).output();
    let cmd = ps
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    let cmd_text = cmd.clone().unwrap_or_default();
    Ok(Some(ListenerInfo {
        pid,
        command_line: cmd,
        is_astral_core: cmd_text.contains("astral-core"),
        is_legacy: is_legacy_command_line(&cmd_text),
        generation_match: generation_matches(&cmd_text),
    }))
}

pub fn kill_pid(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/F"])
            .status()?;
    }
    #[cfg(unix)]
    {
        let _ = Command::new("kill").arg(pid.to_string()).status()?;
    }
    Ok(())
}

fn kill_stale_astral_listeners(port: u16) -> Result<u32> {
    let Some(listener) = find_listener(port)? else {
        return Ok(0);
    };
    if !listener.is_astral_core {
        return Ok(0);
    }
    if listener.generation_match && !listener.is_legacy {
        return Ok(0);
    }
    info!(pid = listener.pid, "终止旧参数 astral-core 监听进程");
    kill_pid(listener.pid)?;
    thread::sleep(Duration::from_millis(400));
    Ok(1)
}

fn stop_service_by_name(name: &str, user: bool) -> Result<bool> {
    let label = ServiceLabel::from_str(name)
        .map_err(|e| anyhow::anyhow!("无效服务标签 {name}: {e}"))?;
    let manager = native_manager(user)?;
    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::NotInstalled) => return Ok(false),
        Err(_) => return Ok(false),
        _ => {}
    }
    let _ = manager.stop(ServiceStopCtx {
        label: label.clone(),
    });
    Ok(true)
}

fn uninstall_service_by_name(name: &str, user: bool) -> Result<bool> {
    let label = ServiceLabel::from_str(name)
        .map_err(|e| anyhow::anyhow!("无效服务标签 {name}: {e}"))?;
    let manager = native_manager(user)?;
    match manager.status(ServiceStatusCtx {
        label: label.clone(),
    }) {
        Ok(ServiceStatus::NotInstalled) => return Ok(false),
        Err(_) => return Ok(false),
        _ => {}
    }
    let _ = manager.stop(ServiceStopCtx {
        label: label.clone(),
    });
    manager.uninstall(ServiceUninstallCtx { label })?;
    Ok(true)
}

fn copy_dir_contents(src: &Path, dest: &Path) -> Result<()> {
    if !src.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let name = entry.file_name();
        let to = dest.join(&name);
        if file_type.is_dir() {
            copy_dir_contents(&entry.path(), &to)?;
        } else if file_type.is_file() {
            if !to.exists() {
                fs::copy(entry.path(), &to)?;
            }
        }
    }
    Ok(())
}

pub fn migrate_legacy_data_if_needed(force: bool) -> Result<bool> {
    let root = default_data_root()?;
    let legacy = legacy_data_dir(&root);
    if !legacy.exists() {
        return Ok(false);
    }
    let node_id = root.join("node_id");
    if node_id.exists() && !force {
        info!("数据根已有 node_id，跳过 legacy 迁移");
        return Ok(false);
    }
    info!(
        from = %legacy.display(),
        to = %root.display(),
        "迁移 legacy 数据目录"
    );
    copy_dir_contents(&legacy, &root)?;
    Ok(true)
}

/// 数据已在根目录时，删除空的 legacy `instances/default` 目录。
pub fn remove_legacy_data_dir_if_safe() -> Result<bool> {
    let root = default_data_root()?;
    let legacy = legacy_data_dir(&root);
    if !legacy.exists() {
        return Ok(false);
    }
    if !root.join("node_id").exists() && legacy.join("node_id").exists() {
        return Ok(false);
    }
    info!(path = %legacy.display(), "删除 legacy 数据目录");
    fs::remove_dir_all(&legacy)?;
    let instances = root.join("instances");
    if instances.is_dir() {
        if fs::read_dir(&instances)?.next().is_none() {
            let _ = fs::remove_dir(&instances);
        }
    }
    Ok(true)
}

pub fn normalize_registry_generation() -> Result<bool> {
    let mut reg = super::registry::load()?;
    if reg.instances.is_empty() {
        return Ok(false);
    }
    let root = default_data_root()?;
    let mut changed = false;
    if reg.service_generation.as_deref() != Some(SERVICE_GENERATION) {
        reg.service_generation = Some(SERVICE_GENERATION.to_string());
        changed = true;
    }
    for inst in &mut reg.instances {
        if inst.name != super::SERVICE_REGISTRY_KEY {
            inst.name = super::SERVICE_REGISTRY_KEY.to_string();
            changed = true;
        }
        if inst.data_dir != root {
            inst.data_dir = root.clone();
            changed = true;
        }
    }
    if changed {
        super::registry::save_raw(&reg)?;
    }
    Ok(changed)
}

pub fn repair_environment(opts: RepairOptions) -> Result<RepairReport> {
    begin_phase(
        MigrationPhase::StopOld,
        None,
        Some("repair".into()),
    )?;
    let mut report = RepairReport::default();
    let listen = default_listen();

    report.stopped_current_service =
        stop_service_by_name(SERVICE_QUALIFIED_NAME, opts.user).unwrap_or(false);

    if uninstall_service_by_name(LEGACY_SERVICE_QUALIFIED_NAME, opts.user)? {
        report.uninstalled_legacy_service = true;
    }

    report.killed_stale_listeners = kill_stale_astral_listeners(listen.port())?;

    if opts.migrate_legacy_data {
        report.migrated_legacy_data = migrate_legacy_data_if_needed(false)?;
    }

    report.removed_legacy_data_dir = remove_legacy_data_dir_if_safe().unwrap_or(false);

    report.normalized_registry = normalize_registry_generation().unwrap_or(false);

    if let Ok(report_health) = super::health::inspect_health(opts.user) {
        let _ = verify_service_consistency(&report_health);
    }

    begin_phase(MigrationPhase::Done, None, None)?;
    clear_state()?;
    Ok(report)
}

/// install / update 前置：清理旧服务、旧进程、迁移数据。
pub fn prepare_install_or_update(user: bool, migrate_data: bool) -> Result<()> {
    resume_if_incomplete(|phase| {
        warn!(phase = ?phase, "检测到未完成迁移，继续执行修复");
        Ok(())
    })?;

    if let Some(state) = super::recovery::load_state()? {
        if state.phase != MigrationPhase::Done {
            warn!(
                phase = ?state.phase,
                "检测到未完成的迁移状态，将继续修复"
            );
        }
    }
    begin_phase(
        MigrationPhase::Preflight,
        None,
        Some("prepare".into()),
    )?;
    let _ = repair_environment(RepairOptions {
        user,
        migrate_legacy_data: migrate_data,
    })?;
    begin_phase(MigrationPhase::MigrateData, None, None)?;
    Ok(())
}

pub fn cleanup_after_uninstall(user: bool, purge_data: bool) -> Result<()> {
    let listen = default_listen();
    let _ = kill_stale_astral_listeners(listen.port())?;
    let _ = uninstall_service_by_name(LEGACY_SERVICE_QUALIFIED_NAME, user)?;
    if purge_data {
        let root = default_data_root()?;
        if root.exists() {
            warn!(path = %root.display(), "purge-data 删除数据根");
            let _ = fs::remove_dir_all(&root);
        }
    }
    let _ = clear_state();
    Ok(())
}

pub fn verify_service_consistency(report: &ServiceHealthReport) -> Result<()> {
    if report.needs_repair {
        warn!(issues = ?report.issues, "服务环境仍需修复");
    }
    Ok(())
}
