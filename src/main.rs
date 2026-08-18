//! astral-core 可执行入口：前台运行或系统服务管理。

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use service_manager::ServiceStatus;
use tracing::info;
use tracing_subscriber::EnvFilter;

use astral_core::config::require_local_listen;
use astral_core::service::{
    self, InstallOptions, RunParams, SERVICE_INSTANCE_NAME, ServiceActionOptions, UpdateOptions,
};

/// Astral 本机内核：嵌入 EasyTier，对本机 GUI 提供 JSON-RPC。
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
    /// 前台运行 JSON-RPC 服务
    Run(RunCli),
    /// 系统服务安装 / 启停（本机单例）
    Service {
        #[command(subcommand)]
        action: ServiceCommand,
    },
}

#[derive(Debug, Clone, Parser)]
struct RunCli {
    /// JSON-RPC 监听地址（仅本机；`0.0.0.0` 会改写为 `127.0.0.1`）
    #[arg(long, env = "ASTRAL_CORE_LISTEN", default_value = "127.0.0.1:50051")]
    listen: SocketAddr,

    /// 数据目录；默认使用平台应用数据目录
    #[arg(long, env = "ASTRAL_CORE_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// 日志过滤（如 info,astral_core=debug）
    #[arg(long, env = "RUST_LOG", default_value = "info")]
    log: String,

    /// Windows：由 SCM 拉起时使用（安装服务时自动写入，勿手动调用）
    #[arg(long, value_name = "SERVICE_NAME", hide = true)]
    windows_service: Option<String>,
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// 注册为系统服务（落入并排版本布局，服务指向 current）
    Install {
        /// JSON-RPC 监听地址（仅本机）
        #[arg(long, default_value = "127.0.0.1:50051")]
        listen: SocketAddr,

        /// 数据目录；缺省为平台数据目录下 instances/default
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
    },
    /// 卸载系统服务
    Uninstall {
        #[arg(long)]
        user: bool,
    },
    /// 启动已安装服务
    Start {
        #[arg(long)]
        user: bool,
    },
    /// 停止服务
    Stop {
        #[arg(long)]
        user: bool,
    },
    /// 查询服务状态
    Status {
        #[arg(long)]
        user: bool,
    },
    /// 落入新版本并重启本机服务
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

        /// 保留版本数（含当前）
        #[arg(long, default_value_t = 3)]
        retain: usize,

        /// 只切换不启动
        #[arg(long)]
        no_start: bool,
    },
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
    }
}

fn init_cli_logging() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .with_target(false)
        .try_init();
}

fn run_entry(run: RunCli) -> anyhow::Result<()> {
    let listen = require_local_listen(run.listen)?;
    let params = RunParams {
        listen,
        data_dir: run.data_dir,
        log: run.log,
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

fn default_action(user: bool) -> ServiceActionOptions {
    ServiceActionOptions {
        name: SERVICE_INSTANCE_NAME.to_string(),
        user,
    }
}

fn service_entry(action: ServiceCommand) -> anyhow::Result<()> {
    match action {
        ServiceCommand::Install {
            listen,
            data_dir,
            program,
            install_root,
            version,
            retain,
            user,
            no_start,
        } => {
            service::install(InstallOptions {
                listen: require_local_listen(listen)?,
                data_dir,
                program,
                install_root,
                version,
                retain,
                user,
                start_after_install: !no_start,
            })?;
        }
        ServiceCommand::Uninstall { user } => {
            service::uninstall(default_action(user))?;
        }
        ServiceCommand::Start { user } => {
            service::start(default_action(user))?;
        }
        ServiceCommand::Stop { user } => {
            service::stop(default_action(user))?;
        }
        ServiceCommand::Status { user } => {
            let meta = service::status_label(SERVICE_INSTANCE_NAME)?;
            let st = service::status(default_action(user))?;
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
            retain,
            no_start,
        } => {
            service::update(UpdateOptions {
                program,
                version,
                install_root,
                retain,
                no_start,
            })?;
        }
    }
    Ok(())
}
