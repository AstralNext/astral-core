//! 场景：鉴权

use crate::harness::TestServer;
use astral_core::pb::PingRequest;
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s01_no_token_unauthenticated() {
    let server = TestServer::start().await;
    let mut bare = server.bare_system().await;
    let err = bare
        .ping(Request::new(PingRequest {}))
        .await
        .expect_err("无 token 应失败");
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s01_bad_token_unauthenticated() {
    let server = TestServer::start().await;
    let mut bad = server.system_with_token("ask_deadbeef_invalid").await;
    let err = bad
        .ping(Request::new(PingRequest {}))
        .await
        .expect_err("错 token 应失败");
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s01_valid_token_ok() {
    let server = TestServer::start().await;
    let mut sys = server.system().await;
    let resp = sys
        .ping(Request::new(PingRequest {}))
        .await
        .expect("合法 token");
    assert!(resp.into_inner().ok);
}
