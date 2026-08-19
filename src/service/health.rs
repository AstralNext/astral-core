//! 服务体检：统一收集 SCM、端口、进程、登记与目录状态。
#![allow(missing_docs)]

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use service_manager::ServiceStatus;

use std::str::FromStr;

use super::manage::{native_manager, status, ServiceActionOptions};
use super::registry;
use super::{LEGACY_SERVICE_QUALIFIED_NAME, SERVICE_GENERATION, SERVICE_QUALIFIED_NAME};

const DEFAULT_LISTEN: &str = "127.0.0.1:50051";

/// 端口监听者信息。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListenerInfo {
    pub pid: u32,
    pub command_line: Option<String>,
    pub is_astral_core: bool,
    pub is_legacy: bool,
    pub generation_match: bool,
}

/// 服务体检报告（GUI / CLI 共用）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServiceHealthReport {
    pub service_name: String,
    pub service_generation: String,
    pub listen: String,
    pub scm_status: String,
    pub legacy_service_installed: bool,
    pub legacy_service_status: Option<String>,
    pub listener: Option<ListenerInfo>,
    pub registry_present: bool,
    pub registry_generation: Option<String>,
    pub registry_data_dir: Option<PathBuf>,
    pub data_dir: PathBuf,
    pub legacy_data_dir: PathBuf,
    pub legacy_data_dir_exists: bool,
    pub issues: Vec<String>,
    pub needs_repair: bool,
}

/// 读取平台默认数据根。
pub fn default_data_root() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("ASTRAL_CORE_DATA_DIR") {
        let path = PathBuf::from(p);
        if !path.as_os_str().is_empty() {
            return Ok(path);
        }
    }
    let dirs = ProjectDirs::from("dev", "Astral", "astral-core")
        .ok_or_else(|| anyhow::anyhow!("无法解析平台数据目录"))?;
    Ok(dirs.data_dir().to_path_buf())
}

fn legacy_data_dir(root: &Path) -> PathBuf {
    root.join("instances").join("default")
}

fn default_listen() -> SocketAddr {
    DEFAULT_LISTEN.parse().expect("valid listen")
}

fn scm_status_text(st: &ServiceStatus) -> &'static str {
    match st {
        ServiceStatus::NotInstalled => "not-installed",
        ServiceStatus::Running => "running",
        ServiceStatus::Stopped(_) => "stopped",
    }
}

fn legacy_service_status(user: bool) -> Result<Option<String>> {
    let label = service_manager::ServiceLabel::from_str(LEGACY_SERVICE_QUALIFIED_NAME)
        .map_err(|e| anyhow::anyhow!("无效旧服务标签: {e}"))?;
    let manager = native_manager(user)?;
    match manager.status(service_manager::ServiceStatusCtx { label }) {
        Ok(st) => Ok(Some(scm_status_text(&st).to_string())),
        Err(_) => Ok(None),
    }
}

fn legacy_service_installed(user: bool) -> bool {
    legacy_service_status(user)
        .ok()
        .flatten()
        .is_some_and(|s| s != "not-installed")
}

pub fn inspect_health(user: bool) -> Result<ServiceHealthReport> {
    let data_dir = default_data_root()?;
    let legacy_data_dir = legacy_data_dir(&data_dir);
    let listen = default_listen();

    let scm = status(ServiceActionOptions { user })?;
    let scm_status = scm_status_text(&scm).to_string();

    let reg = registry::load().ok();
    let registry_present = reg.as_ref().is_some_and(|r| !r.instances.is_empty());
    let registry_generation = reg.as_ref().and_then(|r| r.service_generation.clone());
    let registry_data_dir = reg
        .as_ref()
        .and_then(|r| r.instances.first())
        .map(|i| i.data_dir.clone());

    let legacy_installed = legacy_service_installed(user);
    let legacy_status = if legacy_installed {
        legacy_service_status(user)?
    } else {
        None
    };

    let listener = super::cleanup::find_listener(listen.port())?;

    let mut issues = Vec::new();
    if legacy_installed {
        issues.push(format!("旧服务仍存在: {LEGACY_SERVICE_QUALIFIED_NAME}"));
    }
    if legacy_data_dir.exists() {
        issues.push(format!(
            "旧数据目录仍存在: {}",
            legacy_data_dir.display()
        ));
    }
    if let Some(listener) = &listener {
        if listener.is_astral_core && (!listener.generation_match || listener.is_legacy) {
            issues.push(format!(
                "端口 {} 被旧参数 astral-core 占用 (pid={})",
                listen.port(),
                listener.pid
            ));
        } else if listener.is_astral_core && scm_status != "running" {
            issues.push(format!(
                "端口 {} 有 astral-core 监听，但 SCM 状态为 {scm_status}",
                listen.port()
            ));
        }
    }
    if registry_present {
        if registry_generation.as_deref() != Some(SERVICE_GENERATION) {
            issues.push("登记文件代际过旧或缺失".into());
        }
        if let Some(dir) = &registry_data_dir {
            if dir != &data_dir {
                issues.push(format!(
                    "登记 data_dir 与当前默认不一致: {}",
                    dir.display()
                ));
            }
        }
    }

    let needs_repair = !issues.is_empty();

    Ok(ServiceHealthReport {
        service_name: SERVICE_QUALIFIED_NAME.to_string(),
        service_generation: SERVICE_GENERATION.to_string(),
        listen: listen.to_string(),
        scm_status,
        legacy_service_installed: legacy_installed,
        legacy_service_status: legacy_status,
        listener,
        registry_present,
        registry_generation,
        registry_data_dir,
        data_dir,
        legacy_data_dir: legacy_data_dir.clone(),
        legacy_data_dir_exists: legacy_data_dir.exists(),
        issues,
        needs_repair,
    })
}

pub fn health_report_json(user: bool) -> Result<String> {
    let report = inspect_health(user)?;
    Ok(serde_json::to_string_pretty(&report)?)
}
