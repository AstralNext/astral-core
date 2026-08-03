//! astral-core 可执行入口：前台运行或系统服务管理。

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use service_manager::ServiceStatus;
use tracing::info;
use tracing_subscriber::EnvFilter;

use astral_core::controller::{self, ControllerListenParams};
use astral_core::service::{
    self, InstallOptions, RollbackOptions, RunParams, ServiceActionOptions, UpdateOptions,
};

/// Astral 节点 Core：嵌入 EasyTier，对外提供 astral.v1 gRPC。
#[derive(Debug, Parser)]
#[command(name = "astral-core", version, about, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[command(flatten)]
    run: RunCli,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// 前台运行 gRPC 服务
    Run(RunCli),
    /// 系统服务安装 / 启停（多实例）
    Service {
        #[command(subcommand)]
        action: ServiceCommand,
    },
    /// 控制端：listen 收节点出站 Agent，并可按 x-astral-node-id 隧道代理 RPC
    Controller {
        #[command(subcommand)]
        action: ControllerCommand,
    },
    /// 本地部署 TUI 向导（键盘操作安装 / 启停 / 更新）
    Wizard,
}

#[derive(Debug, Clone, Parser)]
struct RunCli {
    /// gRPC 监听地址
    #[arg(long, env = "ASTRAL_CORE_LISTEN", default_value = "127.0.0.1:50051")]
    listen: SocketAddr,

    /// 数据目录（tokens / node_id）；默认使用平台应用数据目录
    #[arg(long, env = "ASTRAL_CORE_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// 日志过滤（如 info,astral_core=debug）
    #[arg(long, env = "RUST_LOG", default_value = "info")]
    log: String,

    /// Windows：由 SCM 拉起时使用（安装服务时自动写入，勿手动调用）
    #[arg(long, value_name = "SERVICE_NAME", hide = true)]
    windows_service: Option<String>,

    /// 出站连接控制端（与本地 --listen 并存），如 http://1.2.3.4:8443 或 https://...
    #[arg(long, env = "ASTRAL_CORE_CONTROLLER")]
    controller: Option<String>,

    /// 控制端共享密钥（join + attestation）；与 controller listen --token 一致
    #[arg(long, env = "ASTRAL_CORE_CONTROLLER_TOKEN")]
    controller_token: Option<String>,

    /// 控制端 TLS CA / 自签证书 PEM（https 且非公网 CA 时需要）
    #[arg(long, env = "ASTRAL_CORE_CONTROLLER_TLS_CA")]
    controller_tls_ca: Option<PathBuf>,

    /// 控制端 TLS 校验域名 / SNI（URL 为 IP 时可指定证书 CN）
    #[arg(long, env = "ASTRAL_CORE_CONTROLLER_TLS_DOMAIN")]
    controller_tls_domain: Option<String>,
}

#[derive(Debug, Subcommand)]
enum ControllerCommand {
    /// 监听节点主动连接
    Listen {
        /// 绑定地址
        #[arg(long, default_value = "0.0.0.0:8443")]
        bind: SocketAddr,

        /// 与节点共享的密钥（节点 --controller-token）
        #[arg(long, env = "ASTRAL_CONTROLLER_TOKEN")]
        token: String,

        /// 控制端数据目录（设备凭证库）
        #[arg(long)]
        data_dir: Option<PathBuf>,

        /// 日志过滤
        #[arg(long, env = "RUST_LOG", default_value = "info")]
        log: String,

        /// TLS 证书 PEM（与 --tls-key 同时提供；公网强烈建议）
        #[arg(long, env = "ASTRAL_CONTROLLER_TLS_CERT")]
        tls_cert: Option<PathBuf>,

        /// TLS 私钥 PEM
        #[arg(long, env = "ASTRAL_CONTROLLER_TLS_KEY")]
        tls_key: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// 注册为系统服务（先落入并排版本布局，服务指向 current）
    Install {
        /// 实例名（多实例时区分；服务标签为 dev.astral.core-<name>）
        #[arg(long, default_value = "default")]
        name: String,

        /// gRPC 监听地址
        #[arg(long, default_value = "127.0.0.1:50051")]
        listen: SocketAddr,

        /// 数据目录；缺省为平台数据目录下 instances/<name>
        #[arg(long)]
        data_dir: Option<PathBuf>,

        /// 源二进制；缺省为当前 astral-core（会复制进版本目录）
        #[arg(long)]
        program: Option<PathBuf>,

        /// 并排版本安装根；缺省为平台 Local 数据目录下 app/
        #[arg(long)]
        install_root: Option<PathBuf>,

        /// 版本号；缺省从二进制 --version 推断
        #[arg(long)]
        version: Option<String>,

        /// 保留版本数（含当前）
        #[arg(long, default_value_t = 3)]
        retain: usize,

        /// 用户级服务（systemd --user / LaunchAgent）；Windows 不支持
        #[arg(long)]
        user: bool,

        /// 只安装不启动
        #[arg(long)]
        no_start: bool,

        /// 出站控制端 URL（写入服务参数，与本地 --listen 并存）
        #[arg(long)]
        controller: Option<String>,

        /// 控制端共享密钥（与 controller listen --token 一致）
        #[arg(long)]
        controller_token: Option<String>,

        /// 控制端 TLS CA（写入服务参数）
        #[arg(long)]
        controller_tls_ca: Option<PathBuf>,

        /// 控制端 TLS 域名 / SNI
        #[arg(long)]
        controller_tls_domain: Option<String>,
    },
    /// 卸载系统服务
    Uninstall {
        #[arg(long, default_value = "default")]
        name: String,
        #[arg(long)]
        user: bool,
    },
    /// 启动已安装服务
    Start {
        #[arg(long, default_value = "default")]
        name: String,
        #[arg(long)]
        user: bool,
    },
    /// 停止服务
    Stop {
        #[arg(long, default_value = "default")]
        name: String,
        #[arg(long)]
        user: bool,
    },
    /// 查询服务状态
    Status {
        #[arg(long, default_value = "default")]
        name: String,
        #[arg(long)]
        user: bool,
    },
    /// 产品级更新：新版本目录 + 切换 current + 重启实例
    Update {
        /// 新二进制；缺省为当前进程
        #[arg(long)]
        program: Option<PathBuf>,

        /// 版本号；缺省从二进制推断
        #[arg(long)]
        version: Option<String>,

        /// 安装根；缺省用登记值
        #[arg(long)]
        install_root: Option<PathBuf>,

        /// 只重启指定实例（可重复）
        #[arg(long = "name")]
        names: Vec<String>,

        /// 保留版本数（含当前）
        #[arg(long, default_value_t = 3)]
        retain: usize,

        /// 只切换不启动
        #[arg(long)]
        no_start: bool,
    },
    /// 回滚到旧版本（切换 current）
    Rollback {
        /// 目标版本；缺省为上一版本
        #[arg(long)]
        version: Option<String>,
        #[arg(long = "name")]
        names: Vec<String>,
        #[arg(long)]
        no_start: bool,
    },
    /// 列出已安装版本（* 为当前）
    Versions,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => run_entry(cli.run),
        Some(Command::Run(run)) => run_entry(run),
        Some(Command::Service { action }) => {
            init_cli_logging();
            service_entry(action)
        }
        Some(Command::Controller { action }) => controller_entry(action),
        Some(Command::Wizard) => {
            astral_core::wizard::run_wizard()?;
            Ok(())
        }
    }
}

fn init_cli_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with_target(false)
        .try_init();
}

fn run_entry(run: RunCli) -> anyhow::Result<()> {
    let params = RunParams {
        listen: run.listen,
        data_dir: run.data_dir,
        log: run.log,
        controller: run.controller,
        controller_token: run.controller_token,
        controller_tls_ca: run.controller_tls_ca,
        controller_tls_domain: run.controller_tls_domain,
    };

    #[cfg(windows)]
    if let Some(name) = run.windows_service {
        return service::run_as_windows_service(name, params);
    }
    #[cfg(not(windows))]
    if run.windows_service.is_some() {
        anyhow::bail!("--windows-service 仅在 Windows 上可用");
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(service::run_foreground(params))?;
    Ok(())
}

fn controller_entry(action: ControllerCommand) -> anyhow::Result<()> {
    match action {
        ControllerCommand::Listen {
            bind,
            token,
            data_dir,
            log,
            tls_cert,
            tls_key,
        } => {
            let _ = tracing_subscriber::fmt()
                .with_env_filter(EnvFilter::new(&log))
                .with_target(true)
                .try_init();
            let data_dir = match data_dir {
                Some(p) => p,
                None => {
                    let dirs = directories::ProjectDirs::from("dev", "Astral", "astral-controller")
                        .ok_or_else(|| anyhow::anyhow!("无法解析控制端数据目录"))?;
                    dirs.data_dir().to_path_buf()
                }
            };
            let tls = astral_core::tls_util::ServerTlsPaths::from_opts(tls_cert, tls_key)?;
            let params = ControllerListenParams {
                bind,
                token,
                data_dir,
                tls,
            };
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(async {
                controller::run_controller(params, service::shutdown_signal()).await
            })?;
        }
    }
    Ok(())
}

fn service_entry(action: ServiceCommand) -> anyhow::Result<()> {
    match action {
        ServiceCommand::Install {
            name,
            listen,
            data_dir,
            program,
            install_root,
            version,
            retain,
            user,
            no_start,
            controller,
            controller_token,
            controller_tls_ca,
            controller_tls_domain,
        } => {
            service::install(InstallOptions {
                name,
                listen,
                data_dir,
                program,
                install_root,
                version,
                retain,
                user,
                start_after_install: !no_start,
                controller,
                controller_token,
                controller_tls_ca,
                controller_tls_domain,
            })?;
        }
        ServiceCommand::Uninstall { name, user } => {
            service::uninstall(ServiceActionOptions { name, user })?;
        }
        ServiceCommand::Start { name, user } => {
            service::start(ServiceActionOptions { name, user })?;
        }
        ServiceCommand::Stop { name, user } => {
            service::stop(ServiceActionOptions { name, user })?;
        }
        ServiceCommand::Status { name, user } => {
            let meta = service::status_label(&name)?;
            let st = service::status(ServiceActionOptions { name, user })?;
            let text = match &st {
                ServiceStatus::NotInstalled => "not-installed",
                ServiceStatus::Running => "running",
                ServiceStatus::Stopped(reason) => match reason {
                    Some(r) => {
                        info!(reason = %r, "stopped");
                        "stopped"
                    }
                    None => "stopped",
                },
            };
            println!("{text} ({meta})");
        }
        ServiceCommand::Update {
            program,
            version,
            install_root,
            names,
            retain,
            no_start,
        } => {
            service::update(UpdateOptions {
                program,
                version,
                install_root,
                names: if names.is_empty() { None } else { Some(names) },
                retain,
                no_start,
            })?;
        }
        ServiceCommand::Rollback {
            version,
            names,
            no_start,
        } => {
            service::rollback(RollbackOptions {
                version,
                names: if names.is_empty() { None } else { Some(names) },
                no_start,
            })?;
        }
        ServiceCommand::Versions => {
            println!("{}", service::list_versions_report()?);
        }
    }
    Ok(())
}
