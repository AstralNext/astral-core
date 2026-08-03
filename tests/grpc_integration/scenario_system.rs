//! 场景：SystemService

use crate::harness::TestServer;
use astral_core::pb::{GetCapabilitiesRequest, GetServerInfoRequest, PingRequest, ServerMode};
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s02_ping_info_capabilities() {
    let server = TestServer::start().await;
    let mut sys = server.system().await;

    assert!(sys
        .ping(Request::new(PingRequest {}))
        .await
        .unwrap()
        .into_inner()
        .ok);

    let info = sys
        .get_server_info(Request::new(GetServerInfoRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(info.protocol_package, "astral.v1");
    assert_eq!(info.mode, ServerMode::Node as i32);
    assert!(!info.api_version.is_empty());
    assert!(info.capabilities.iter().any(|c| c.contains("system")));

    let caps = sys
        .get_capabilities(Request::new(GetCapabilitiesRequest {}))
        .await
        .unwrap()
        .into_inner();
    assert!(caps.services.iter().any(|s| s.contains("System")));
    assert!(caps.features.iter().any(|f| f.contains("auth")));
}
