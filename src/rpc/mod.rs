//! 本机 HTTP JSON-RPC 2.0（仅 loopback）。

mod handlers;

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tracing::info;

use crate::app::AppState;
use crate::error::{CoreError, CoreResult};

/// 在配置的地址上启动 JSON-RPC，直到进程被取消。
pub async fn serve(state: AppState) -> CoreResult<()> {
    let addr = state.runtime.listen;
    info!(%addr, "启动 astral-core JSON-RPC");
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| CoreError::Internal(format!("监听失败: {e}")))?;
    axum::serve(listener, router(state))
        .await
        .map_err(|e| CoreError::Internal(format!("JSON-RPC 服务失败: {e}")))?;
    Ok(())
}

/// 在配置的地址上启动 JSON-RPC，并在 `shutdown` 完成时退出。
pub async fn serve_with_shutdown<F>(state: AppState, shutdown: F) -> CoreResult<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let addr = state.runtime.listen;
    info!(%addr, "启动 astral-core JSON-RPC（可关闭）");
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| CoreError::Internal(format!("监听失败: {e}")))?;
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| CoreError::Internal(format!("JSON-RPC 服务失败: {e}")))?;
    Ok(())
}

/// 绑定已有 [`TcpListener`]（可为 `127.0.0.1:0`），并在 `shutdown` 完成时退出。
pub async fn serve_with_incoming_shutdown<F>(
    state: AppState,
    listener: TcpListener,
    shutdown: F,
) -> CoreResult<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let addr = listener
        .local_addr()
        .map_err(|e| CoreError::Internal(e.to_string()))?;
    info!(%addr, "启动 astral-core JSON-RPC（可关闭）");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown)
        .await
        .map_err(|e| CoreError::Internal(format!("JSON-RPC 服务失败: {e}")))?;
    Ok(())
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", post(rpc_entry))
        .route("/rpc", post(rpc_entry))
        .with_state(state)
}

#[derive(Deserialize)]
struct RpcRequest {
    #[serde(default)]
    id: Value,
    #[serde(default)]
    method: String,
    #[serde(default)]
    params: Value,
}

async fn rpc_entry(State(state): State<AppState>, body: String) -> Json<Value> {
    let req: RpcRequest = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            return Json(rpc_error(Value::Null, -32700, format!("JSON 解析失败: {e}")));
        }
    };
    let id = if req.id.is_null() {
        Value::Null
    } else {
        req.id
    };
    if req.method.is_empty() {
        return Json(rpc_error(id, -32600, "缺少 method".into()));
    }
    match handlers::dispatch(&state, &req.method, req.params).await {
        Ok(result) => Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        })),
        Err(e) => Json(rpc_error(id, e.rpc_code(), e.to_string())),
    }
}

fn rpc_error(id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
}
