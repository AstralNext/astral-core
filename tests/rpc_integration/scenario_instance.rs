//! 场景：实例启停与查询

use crate::harness::{rpc_call, try_start_smoke, TestServer};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s04_lifecycle_start_get_list_meta_stop() {
    let server = TestServer::start().await;
    let Some(id) = try_start_smoke(&server).await else {
        return;
    };

    let got = rpc_call(&server.addr, "instance.get", json!({ "instance_id": id }))
        .await
        .expect("get");
    assert_eq!(
        got.pointer("/summary/instance_id").and_then(|v| v.as_str()),
        Some(id.as_str())
    );

    let meta = rpc_call(&server.addr, "instance.list_meta", json!({}))
        .await
        .unwrap();
    let metas = meta
        .get("metas")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(metas
        .iter()
        .any(|m| m.get("instance_id").and_then(|v| v.as_str()) == Some(id.as_str())));

    let _ = rpc_call(&server.addr, "instance.stop", json!({ "instance_id": id }))
        .await
        .expect("stop");

    let _ = rpc_call(&server.addr, "instance.stop", json!({ "instance_id": id }))
        .await
        .expect("stop idempotent");
}
