//! 场景：ping / info

use crate::harness::{rpc_call, TestServer};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s02_ping_info() {
    let server = TestServer::start().await;

    let ping = rpc_call(&server.addr, "ping", json!({})).await.unwrap();
    assert_eq!(ping.get("ok").and_then(|v| v.as_bool()), Some(true));

    let info = rpc_call(&server.addr, "info", json!({})).await.unwrap();
    assert_eq!(
        info.get("protocol").and_then(|v| v.as_str()),
        Some("astral.jsonrpc.v1")
    );
    assert!(!info
        .get("core_version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .is_empty());
}
