//! 前台 / 服务共用的运行与关闭逻辑。

use std::net::SocketAddr;
use std::path::PathBuf;

use tracing::{info, warn};

use crate::app::AppState;
use crate::config::{require_local_listen, RuntimeConfigBuilder};
use crate::error::CoreResult;
use crate::rpc;

/// 前台或服务模式下的运行参数。
#[derive(Debug, Clone)]
pub struct RunParams {
    /// JSON-RPC 监听地址（仅本机）。
    pub listen: SocketAddr,
    /// 可选数据目录覆盖。
    pub data_dir: Option<PathBuf>,
    /// 日志过滤表达式。
    pub log: String,
}

/// 初始化日志、引导状态并返回 [`AppState`]。
pub fn bootstrap_runtime(params: &RunParams) -> CoreResult<AppState> {
    let listen = require_local_listen(params.listen)?;
    let mut builder = RuntimeConfigBuilder::new().listen(listen);
    if let Some(dir) = params.data_dir.clone() {
        builder = builder.data_dir(dir);
    }
    let runtime = builder.build()?;

    let state = AppState::bootstrap_with_log_filter(runtime, &params.log)?;
    info!(
        node_id = %state.node_id,
        data_dir = %state.paths.root.display(),
        listen = %state.runtime.listen,
        "astral-core 已引导"
    );
    Ok(state)
}

/// 前台运行：本机 JSON-RPC；捕获信号后退出。
pub async fn run_foreground(params: RunParams) -> CoreResult<()> {
    let state = bootstrap_runtime(&params)?;
    rpc::serve_with_shutdown(state, shutdown_signal()).await
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
