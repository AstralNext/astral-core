//! 场景：NodeService

use crate::harness::{assert_unimplemented, TestServer};
use astral_core::pb::{
    GenerateNodeEnrollTokenRequest, GetNodeHostInfoRequest, GetNodeRequest, GetSelfNodeRequest,
    ListNodesRequest, RenameNodeRequest,
};
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s05_self_list_get_hostinfo() {
    let server = TestServer::start().await;
    let mut node = server.node().await;

    let self_n = node
        .get_self_node(Request::new(GetSelfNodeRequest {}))
        .await
        .unwrap()
        .into_inner()
        .node
        .expect("self node");
    assert!(!self_n.node_id.is_empty());
    assert!(self_n.online);

    let list = node
        .list_nodes(Request::new(ListNodesRequest {
            online_only: false,
        }))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(list.nodes.len(), 1);
    assert_eq!(list.nodes[0].node_id, self_n.node_id);

    let got = node
        .get_node(Request::new(GetNodeRequest {
            node_id: self_n.node_id.clone(),
        }))
        .await
        .unwrap()
        .into_inner()
        .node
        .unwrap();
    assert_eq!(got.node_id, self_n.node_id);

    let host = node
        .get_node_host_info(Request::new(GetNodeHostInfoRequest { node: None }))
        .await
        .unwrap()
        .into_inner();
    assert!(!host.os.is_empty());
    assert!(!host.arch.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s05_control_plane_unimplemented() {
    let server = TestServer::start().await;
    let mut node = server.node().await;
    assert_unimplemented(
        &node
            .rename_node(Request::new(RenameNodeRequest {
                node_id: String::new(),
                name: "x".into(),
            }))
            .await
            .unwrap_err(),
    );
    assert_unimplemented(
        &node
            .generate_node_enroll_token(Request::new(GenerateNodeEnrollTokenRequest {
                ttl_seconds: 60,
                max_uses: 1,
                name_prefix: String::new(),
            }))
            .await
            .unwrap_err(),
    );
}
