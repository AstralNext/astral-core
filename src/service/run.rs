//! 前台 / 服务共用的运行与关闭逻辑。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::agent::{run_agent_loop, AgentConnectConfig};
use crate::app::AppState;
use crate::config::RuntimeConfigBuilder;
use crate::error::CoreResult;
use crate::grpc;
use crate::tls_util::ClientTlsOpts;

/// 前台或服务模式下的运行参数。
#[derive(Debug, Clone)]
pub struct RunParams {
    /// gRPC 监听地址。
    pub listen: SocketAddr,
    /// 可选数据目录覆盖。
    pub data_dir: Option<PathBuf>,
    /// 日志过滤表达式。
    pub log: String,
    /// 可选：出站连接的控制端 URL（如 `https://1.2.3.4:8443`）。
    pub controller: Option<String>,
    /// 与控制端共享的 join / attestation 密钥。
    pub controller_token: Option<String>,
    /// 控制端 TLS：自定义 CA（自签时传服务端证书）。
    pub controller_tls_ca: Option<PathBuf>,
    /// 控制端 TLS：SNI / 校验域名。
    pub controller_tls_domain: Option<String>,
}

/// 初始化日志、引导状态并返回 [`AppState`]。
pub fn bootstrap_runtime(params: &RunParams) -> CoreResult<AppState> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&params.log))
        .with_target(true)
        .try_init();

    let mut builder = RuntimeConfigBuilder::new().grpc_listen(params.listen);
    if let Some(dir) = params.data_dir.clone() {
        builder = builder.data_dir(dir);
    }
    let runtime = builder.build();

    let (state, bootstrap) = AppState::bootstrap(runtime)?;
    info!(
        node_id = %state.node_id,
        data_dir = %state.paths.root.display(),
        listen = %state.runtime.grpc_listen,
        "astral-core 已引导"
    );

    if let Some(token) = bootstrap {
        warn!(
            "已生成引导 API Token（仅此次显示，请妥善保存）: {}",
            token
        );
        warn!(
            "明文副本亦写入: {}",
            state.paths.bootstrap_token_file().display()
        );
    }

    Ok(state)
}

/// 前台运行：本地 gRPC + 可选出站 Agent；捕获信号后退出。
pub async fn run_foreground(params: RunParams) -> CoreResult<()> {
    let state = bootstrap_runtime(&params)?;
    spawn_agent_if_configured(&state, &params)?;
    grpc::serve_with_shutdown(state, shutdown_signal()).await
}

/// 若配置了 controller，则后台启动出站 Agent 循环。
pub fn spawn_agent_if_configured(state: &AppState, params: &RunParams) -> CoreResult<()> {
    let Some(controller) = params.controller.clone() else {
        return Ok(());
    };
    let token = params.controller_token.clone().ok_or_else(|| {
        crate::error::CoreError::InvalidArgument(
            "已设置 --controller 时必须同时提供 --controller-token".into(),
        )
    })?;
    if controller.starts_with("https://") || controller.starts_with("HTTPS://") {
        if let Some(ca) = &params.controller_tls_ca {
            crate::tls_util::require_readable(ca, "--controller-tls-ca")?;
        }
    }
    let cfg = AgentConnectConfig {
        controller,
        token,
        retry_base: Duration::from_secs(3),
        retry_max: Duration::from_secs(30),
        heartbeat_interval: Duration::from_secs(15),
        tls: ClientTlsOpts {
            ca_cert: params.controller_tls_ca.clone(),
            domain: params.controller_tls_domain.clone(),
        },
    };
    let agent_state = state.clone();
    tokio::spawn(async move {
        run_agent_loop(agent_state, cfg).await;
    });
    info!("已启动出站 Agent 循环（与本地 listen 并存）");
    Ok(())
}

/// 等待进程终止信号。
pub async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            warn!(error = %e, "监听 Ctrl+C 失败");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(e) => warn!(error = %e, "监听 SIGTERM 失败"),
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("收到 Ctrl+C，准备退出"),
        _ = terminate => info!("收到终止信号，准备退出"),
    }
}
