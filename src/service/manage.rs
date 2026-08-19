//! 基于 `service-manager` 的安装 / 启停。本机只装 `default` 一槽。

use std::ffi::OsString;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, bail, Context, Result};
use directories::ProjectDirs;
use service_manager::{
    ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx, ServiceStatus,
    ServiceStatusCtx, ServiceStopCtx, ServiceUninstallCtx,
};
use tracing::info;

use super::{SERVICE_GENERATION, SERVICE_QUALIFIED_NAME};

/// 安装选项。
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// gRPC 监听地址（仅本机）。
    pub listen: SocketAddr,
    /// 可选数据目录；缺省为平台数据根目录。
    pub data_dir: Option<PathBuf>,
    /// 要落入布局的源二进制；缺省为当前进程。
    pub program: Option<PathBuf>,
    /// 并排版本安装根；缺省为平台 Local/`app`。
    pub install_root: Option<PathBuf>,
    /// 版本号；缺省从二进制 `--version` 或 crate 版本推断。
    pub version: Option<String>,
    /// 保留版本数（含当前），默认 3。
    pub retain: usize,
    /// 用户级服务（systemd user / launchd agent）；Windows 不支持。
    pub user: bool,
    /// 安装后立即启动。
    pub start_after_install: bool,
}

/// 启停 / 卸载 / 状态查询的公共选项。
#[derive(Debug, Clone)]
pub struct ServiceActionOptions {
    /// 用户级服务。
    pub user: bool,
}

/// 卸载选项。
#[derive(Debug, Clone)]
pub struct UninstallOptions {
    /// 用户级服务。
    pub user: bool,
    /// 删除数据根（危险，需显式指定）。
    pub purge_data: bool,
}

/// 生成服务标签 `dev.astral.core`。
pub fn service_label() -> Result<ServiceLabel> {
    ServiceLabel::from_str(SERVICE_QUALIFIED_NAME)
        .map_err(|e| anyhow!("无效服务标签 {SERVICE_QUALIFIED_NAME}: {e}"))
}

/// 人类可读的服务标识。
pub fn status_label() -> Result<String> {
    let label = service_label()?;
    Ok(format!(
        "qualified={} script={}",
        label.to_qualified_name(),
        label.to_script_name()
    ))
}

pub(crate) fn native_manager(user: bool) -> Result<Box<dyn ServiceManager>> {
    let mut manager = <dyn ServiceManager>::native()
        .context("无法检测本机服务管理器（systemd / launchd / sc.exe）")?;
    if user {
        manager
            .set_level(ServiceLevel::User)
            .context("当前平台服务管理器不支持用户级服务")?;
    }
    Ok(manager)
}

pub(crate) fn resolve_program(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let path = match explicit {
        Some(p) => p,
        None => std::env::current_exe().context("无法解析当前可执行文件路径")?,
    };
    let path = if path.exists() {
        dunce_canonicalize(&path).unwrap_or(path)
    } else {
        path
    };
    Ok(path)
}

/// 不依赖额外 crate 的 canonicalize（Windows 上去掉 `\\?\` 前缀以便 sc.exe）。
pub(crate) fn dunce_canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    let p = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return Ok(PathBuf::from(format!(r"\\{rest}")));
        }
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return Ok(PathBuf::from(stripped));
        }
    }
    Ok(p)
}

fn default_service_data_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "Astral", "astral-core")
        .ok_or_else(|| anyhow!("无法解析平台数据目录"))?;
    Ok(dirs.data_dir().to_path_buf())
}

fn resolve_data_dir(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let dir = match explicit {
        Some(p) => p,
        None => default_service_data_dir()?,
    };
    let dir = if dir.as_os_str().is_empty() {
        bail!("data-dir 不能为空");
    } else if dir.is_absolute() {
        dir
    } else {
        std::env::current_dir()
            .context("无法解析当前工作目录")?
            .join(dir)
    };
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("创建数据目录失败: {}", dir.display()))?;
    Ok(dunce_canonicalize(&dir).unwrap_or(dir))
}

fn build_service_args(listen: SocketAddr, data_dir: &Path, label: &ServiceLabel) -> Vec<OsString> {
    let mut args = Vec::new();
    #[cfg(windows)]
    {
        args.push(OsString::from("--windows-service"));
        args.push(OsString::from(label.to_qualified_name()));
    }
    args.push(OsString::from("--listen"));
    args.push(OsString::from(listen.to_string()));
    args.push(OsString::from("--data-dir"));
    args.push(data_dir.as_os_str().to_os_string());
    args.push(OsString::from("--service-generation"));
    args.push(OsString::from(SERVICE_GENERATION));
    args
}

/// 安装系统服务（先落入并排版本布局，服务指向 `current`）。
pub fn install(opts: InstallOptions) -> Result<()> {
    super::cleanup::prepare_install_or_update(opts.user, true)?;
    super::recovery::begin_phase(
        super::recovery::MigrationPhase::StageNewVersion,
        opts.version.clone(),
        None,
    )?;
    let label = service_label()?;
    let source = resolve_program(opts.program)?;
    let root = super::layout::resolve_install_root(opts.install_root)?;
    let version = super::layout::resolve_version(opts.version.as_deref(), &source)?;
    super::layout::stage_version(&root, &version, &source)?;
    super::layout::switch_current(&root, &version)?;
    super::layout::prune_versions(&root, &version, opts.retain)?;

    let program = super::layout::current_program(&root);
    let data_dir = resolve_data_dir(opts.data_dir)?;
    let args = build_service_args(opts.listen, &data_dir, &label);
    let manager = native_manager(opts.user)?;

    info!(
        service = %label.to_qualified_name(),
        program = %program.display(),
        install_root = %root.display(),
        version = %version,
        data_dir = %data_dir.display(),
        listen = %opts.listen,
        user = opts.user,
        "正在安装 astral-core 服务"
    );

    manager
        .install(ServiceInstallCtx {
            label: label.clone(),
            program: program.clone(),
            args,
            contents: None,
            username: None,
            working_directory: Some(data_dir.clone()),
            environment: None,
            autostart: true,
            disable_restart_on_failure: false,
        })
        .with_context(|| format!("安装服务失败: {}", label.to_qualified_name()))?;

    super::registry::record_install(&root, &version, &program, opts.listen, &data_dir, opts.user)?;

    if opts.start_after_install {
        manager
            .start(ServiceStartCtx {
                label: label.clone(),
            })
            .with_context(|| format!("启动服务失败: {}", label.to_qualified_name()))?;
        info!(service = %label.to_qualified_name(), "服务已启动");
    } else {
        info!(service = %label.to_qualified_name(), "服务已安装（未启动）");
    }

    super::recovery::begin_phase(super::recovery::MigrationPhase::Done, Some(version), None)?;
    let _ = super::recovery::clear_state();
    Ok(())
}

/// 卸载系统服务并清理残留。
pub fn uninstall(opts: UninstallOptions) -> Result<()> {
    uninstall_service_only(ServiceActionOptions { user: opts.user })?;
    super::cleanup::cleanup_after_uninstall(opts.user, opts.purge_data)?;
    Ok(())
}

fn uninstall_service_only(opts: ServiceActionOptions) -> Result<()> {
    let label = service_label()?;
    let manager = native_manager(opts.user)?;
    let _ = manager.stop(ServiceStopCtx {
        label: label.clone(),
    });
    manager
        .uninstall(ServiceUninstallCtx {
            label: label.clone(),
        })
        .with_context(|| format!("卸载服务失败: {}", label.to_qualified_name()))?;
    super::registry::record_uninstall(opts.user).with_context(|| "更新服务登记失败")?;
    info!(service = %label.to_qualified_name(), "服务已卸载");
    Ok(())
}

/// 启动已安装服务。
pub fn start(opts: ServiceActionOptions) -> Result<()> {
    let label = service_label()?;
    let manager = native_manager(opts.user)?;
    manager
        .start(ServiceStartCtx {
            label: label.clone(),
        })
        .with_context(|| format!("启动服务失败: {}", label.to_qualified_name()))?;
    info!(service = %label.to_qualified_name(), "服务已启动");
    Ok(())
}

/// 停止服务。
pub fn stop(opts: ServiceActionOptions) -> Result<()> {
    let label = service_label()?;
    let manager = native_manager(opts.user)?;
    manager
        .stop(ServiceStopCtx {
            label: label.clone(),
        })
        .with_context(|| format!("停止服务失败: {}", label.to_qualified_name()))?;
    info!(service = %label.to_qualified_name(), "服务已停止");
    Ok(())
}

/// 查询服务状态。
pub fn status(opts: ServiceActionOptions) -> Result<ServiceStatus> {
    let label = service_label()?;
    let manager = native_manager(opts.user)?;
    manager
        .status(ServiceStatusCtx { label })
        .context("查询服务状态失败")
}
