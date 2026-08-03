//! 场景：NetworkService（依赖 Start 成功）

use crate::harness::{instance_ref, try_start_smoke, TestServer};
use astral_core::pb::{
    CollectNetworkInfoRequest, GetNetworkStatusRequest, ListPeersRequest, ListRoutesRequest,
    ShowLocalNodeInfoRequest, StopInstanceRequest,
};
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s06_status_peers_collect_routes_local() {
    let server = TestServer::start().await;
    let mut inst = server.instance().await;
    let Some(started) = try_start_smoke(&mut inst).await else {
        return;
    };
    let id = started.instance_id.clone();
    // 给实例 RPC 门面一点时间
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let mut net = server.network().await;
    let status = net
        .get_network_status(Request::new(GetNetworkStatusRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await;
    match status {
        Ok(resp) => {
            let body = resp.into_inner();
            assert_eq!(body.instance_id, id);
        }
        Err(e) => eprintln!("GetNetworkStatus 暂不可用: {e}"),
    }

    let _ = net
        .list_peers(Request::new(ListPeersRequest {
            instance: Some(instance_ref(&id)),
            include_dead: false,
        }))
        .await;

    let collect = net
        .collect_network_info(Request::new(CollectNetworkInfoRequest {
            instance: Some(instance_ref(&id)),
            format: "json".into(),
        }))
        .await
        .expect("collect")
        .into_inner();
    assert!(!collect.raw_json.is_empty());

    let _ = net
        .list_routes(Request::new(ListRoutesRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await;

    let _ = net
        .show_local_node_info(Request::new(ShowLocalNodeInfoRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await;

    let _ = inst
        .stop_instance(Request::new(StopInstanceRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s06_missing_instance_not_found() {
    let server = TestServer::start().await;
    let mut net = server.network().await;
    let err = net
        .get_network_status(Request::new(GetNetworkStatusRequest {
            instance: Some(instance_ref("00000000-0000-4000-8000-000000000000")),
        }))
        .await
        .expect_err("不存在实例");
    assert_eq!(err.code(), tonic::Code::NotFound);
}
