//! 场景：logs.recent

use crate::harness::{rpc_call, TestServer};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s09_recent_logs() {
    let server = TestServer::start().await;

    tracing::warn!(target: "astral_core", "logger-probe-recent");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let recent = rpc_call(
        &server.addr,
        "logs.recent",
        json!({ "after": 0, "limit": 200 }),
    )
    .await
    .expect("logs.recent");
    let lines = recent
        .get("lines")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        lines.iter().any(|l| l
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .contains("logger-probe-recent")),
        "recent={lines:?}"
    );
}
