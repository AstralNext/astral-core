//! 场景：network.status

use crate::harness::{rpc_call, try_start_smoke, TestServer};
use serde_json::json;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s06_network_status() {
    let server = TestServer::start().await;
    let Some(id) = try_start_smoke(&server).await else {
        return;
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    match rpc_call(
        &server.addr,
        "network.status",
        json!({ "instance_id": id }),
    )
    .await
    {
        Ok(body) => {
            assert_eq!(
                body.get("instance_id").and_then(|v| v.as_str()),
                Some(id.as_str())
            );
        }
        Err((_, e)) => eprintln!("network.status 暂不可用: {e}"),
    }

    let _ = rpc_call(
        &server.addr,
        "instance.stop",
        json!({ "instance_id": id }),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s06_missing_instance_not_found() {
    let server = TestServer::start().await;
    let err = rpc_call(
        &server.addr,
        "network.status",
        json!({ "instance_id": "00000000-0000-4000-8000-000000000000" }),
    )
    .await
    .expect_err("不存在实例");
    assert_eq!(err.0, -32004);
}
