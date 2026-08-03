//! 基于 `service-manager` 的安装 / 启停。

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

/// 安装选项。
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// 实例名（多实例时区分服务）。
    pub name: String,
    /// gRPC 监听地址。
    pub listen: SocketAddr,
    /// 可选数据目录；缺省为平台数据目录下 `instances/<name>`。
    pub data_dir: Option<PathBuf>,
    /// 要落入布局的源二进制；缺省为当前进程。
    pub program: Option<PathBuf>,
    /// 并排版本安装根；缺省为平台 Local/`app`。
    pub install_root: Option<PathBuf>,
    /// 版本号；缺省从二进制推断。
    pub version: Option<String>,
    /// 保留版本数（含当前），默认 3。
    pub retain: usize,
    /// 用户级服务（systemd user / launchd agent）；Windows 不支持。
    pub user: bool,
    /// 安装后立即启动。
    pub start_after_install: bool,
    /// 出站控制端 URL（写入服务启动参数）。
    pub controller: Option<String>,
    /// 与控制端共享密钥（写入服务启动参数）。
    pub controller_token: Option<String>,
    /// 控制端 TLS CA PEM。
    pub controller_tls_ca: Option<PathBuf>,
    /// 控制端 TLS 域名 / SNI。
    pub controller_tls_domain: Option<String>,
}

/// 启停 / 卸载 / 状态查询的公共选项。
#[derive(Debug, Clone)]
pub struct ServiceActionOptions {
    /// 实例名。
    pub name: String,
    /// 用户级服务。
    pub user: bool,
}

/// 校验实例名并生成服务标签 `dev.astral.core-<name>`。
pub fn service_label(name: &str) -> Result<ServiceLabel> {
    validate_instance_name(name)?;
    let qualified = format!("dev.astral.core-{name}");
    ServiceLabel::from_str(&qualified).map_err(|e| anyhow!("无效服务标签 {qualified}: {e}"))
}

/// 人类可读的服务标识（systemd unit / Windows 服务名等因平台而异）。
pub fn status_label(name: &str) -> Result<String> {
    let label = service_label(name)?;
    Ok(format!(
        "qualified={} script={}",
        label.to_qualified_name(),
        label.to_script_name()
    ))
}

fn validate_instance_name(name: &str) -> Result<()> {
    if name.is_empty() || name.len() > 64 {
        bail!("实例名长度须为 1..=64");
    }
    let ok = name
        .chars()
        .enumerate()
        .all(|(i, c)| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' => true,
            '-' | '_' => i > 0,
            _ => false,
        });
    if !ok {
        bail!("实例名仅允许字母数字，以及非首位的 -/_：{name}");
    }
    Ok(())
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

/// 实例默认数据目录：`<app-data>/instances/<name>`。
fn default_instance_data_dir(name: &str) -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "Astral", "astral-core")
        .ok_or_else(|| anyhow!("无法解析平台数据目录"))?;
    Ok(dirs.data_dir().join("instances").join(name))
}

fn resolve_data_dir(name: &str, explicit: Option<PathBuf>) -> Result<PathBuf> {
    let dir = match explicit {
        Some(p) => p,
        None => default_instance_data_dir(name)?,
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

fn build_service_args(
    listen: SocketAddr,
    data_dir: &Path,
    label: &ServiceLabel,
    controller: Option<&str>,
    controller_token: Option<&str>,
    controller_tls_ca: Option<&Path>,
    controller_tls_domain: Option<&str>,
) -> Result<Vec<OsString>> {
    if controller.is_some() != controller_token.is_some() {
        bail!("--controller 与 --controller-token 必须同时提供或同时省略");
    }
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
    if let (Some(url), Some(token)) = (controller, controller_token) {
        if url.trim().is_empty() || token.is_empty() {
            bail!("controller / controller-token 不能为空");
        }
        args.push(OsString::from("--controller"));
        args.push(OsString::from(url));
        args.push(OsString::from("--controller-token"));
        args.push(OsString::from(token));
        if let Some(ca) = controller_tls_ca {
            if !ca.is_file() {
                bail!("--controller-tls-ca 不是文件: {}", ca.display());
            }
            args.push(OsString::from("--controller-tls-ca"));
            args.push(ca.as_os_str().to_os_string());
        }
        if let Some(domain) = controller_tls_domain {
            if domain.trim().is_empty() {
                bail!("--controller-tls-domain 不能为空");
            }
            args.push(OsString::from("--controller-tls-domain"));
            args.push(OsString::from(domain));
        }
    } else if controller_tls_ca.is_some() || controller_tls_domain.is_some() {
        bail!("未设置 --controller 时不能单独设置 TLS 选项");
    }
    Ok(args)
}

/// 安装系统服务（先落入并排版本布局，服务指向 `current`）。
pub fn install(opts: InstallOptions) -> Result<()> {
    let label = service_label(&opts.name)?;
    let source = resolve_program(opts.program)?;
    let root = super::layout::resolve_install_root(opts.install_root)?;
    let version = super::layout::resolve_version(opts.version.as_deref(), &source)?;
    super::layout::stage_version(&root, &version, &source)?;
    super::layout::switch_current(&root, &version)?;
    super::layout::prune_versions(&root, &version, opts.retain)?;

    let program = super::layout::current_program(&root);
    let data_dir = resolve_data_dir(&opts.name, opts.data_dir)?;
    let args = build_service_args(
        opts.listen,
        &data_dir,
        &label,
        opts.controller.as_deref(),
        opts.controller_token.as_deref(),
        opts.controller_tls_ca.as_deref(),
        opts.controller_tls_domain.as_deref(),
    )?;
    let manager = native_manager(opts.user)?;

    info!(
        service = %label.to_qualified_name(),
        program = %program.display(),
        install_root = %root.display(),
        version = %version,
        data_dir = %data_dir.display(),
        listen = %opts.listen,
        user = opts.user,
        controller = opts.controller.as_deref().unwrap_or(""),
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

    super::registry::record_install(
        &root,
        &version,
        &program,
        &opts.name,
        opts.listen,
        &data_dir,
        opts.user,
    )?;

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

    Ok(())
}

/// 卸载系统服务。
pub fn uninstall(opts: ServiceActionOptions) -> Result<()> {
    let label = service_label(&opts.name)?;
    let manager = native_manager(opts.user)?;
    let _ = manager.stop(ServiceStopCtx {
        label: label.clone(),
    });
    manager
        .uninstall(ServiceUninstallCtx { label: label.clone() })
        .with_context(|| format!("卸载服务失败: {}", label.to_qualified_name()))?;
    super::registry::record_uninstall(&opts.name, opts.user)
        .with_context(|| format!("更新服务登记失败: {}", opts.name))?;
    info!(service = %label.to_qualified_name(), "服务已卸载");
    Ok(())
}

/// 启动已安装服务。
pub fn start(opts: ServiceActionOptions) -> Result<()> {
    let label = service_label(&opts.name)?;
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
    let label = service_label(&opts.name)?;
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
    let label = service_label(&opts.name)?;
    let manager = native_manager(opts.user)?;
    manager
        .status(ServiceStatusCtx { label })
        .context("查询服务状态失败")
}
