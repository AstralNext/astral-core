//! 场景：明确占位 / 未实现 RPC 契约（必须返回 UNIMPLEMENTED）

use crate::harness::{assert_unimplemented, instance_ref, TestServer};
use astral_core::pb::{
    AppCallRequest, ExportBundleRequest, GetLoggerConfigRequest, GetStatsRequest,
    GetVpnPortalInfoRequest, ListPortForwardsRequest, GetWhitelistRequest,
};
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s09_logger_and_backup_unimplemented() {
    let server = TestServer::start().await;
    let mut logger = server.logger().await;
    assert_unimplemented(
        &logger
            .get_logger_config(Request::new(GetLoggerConfigRequest { node: None }))
            .await
            .unwrap_err(),
    );

    let mut backup = server.backup().await;
    assert_unimplemented(
        &backup
            .export_bundle(Request::new(ExportBundleRequest {
                node: None,
                include_secrets: false,
            }))
            .await
            .unwrap_err(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s09_engine_services_need_running_instance() {
    // 无实例时依赖引擎的接口应 FailedPrecondition / NotFound，而不是 panic
    let server = TestServer::start().await;
    let missing = instance_ref("00000000-0000-4000-8000-000000000099");

    let mut vpn = server.vpn().await;
    let e = vpn
        .get_vpn_portal_info(Request::new(GetVpnPortalInfoRequest {
            instance: Some(missing.clone()),
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(
            e.code(),
            tonic::Code::NotFound | tonic::Code::FailedPrecondition | tonic::Code::Internal
        ),
        "{e}"
    );

    let mut pf = server.portforward().await;
    let e = pf
        .list_port_forwards(Request::new(ListPortForwardsRequest {
            instance: Some(missing.clone()),
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(
            e.code(),
            tonic::Code::NotFound | tonic::Code::FailedPrecondition | tonic::Code::Internal
        ),
        "{e}"
    );

    let mut acl = server.acl().await;
    let e = acl
        .get_whitelist(Request::new(GetWhitelistRequest {
            instance: Some(missing.clone()),
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(
            e.code(),
            tonic::Code::NotFound | tonic::Code::FailedPrecondition | tonic::Code::Internal
        ),
        "{e}"
    );

    let mut stats = server.stats().await;
    let e = stats
        .get_stats(Request::new(GetStatsRequest {
            instance: Some(missing.clone()),
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(
            e.code(),
            tonic::Code::NotFound | tonic::Code::FailedPrecondition | tonic::Code::Internal
        ),
        "{e}"
    );

    let mut app = server.app_message().await;
    let e = app
        .call(Request::new(AppCallRequest {
            instance: Some(missing),
            to_peer_id: "1".into(),
            method: "ping".into(),
            payload: vec![],
            timeout_ms: 100,
        }))
        .await
        .unwrap_err();
    assert!(
        matches!(
            e.code(),
            tonic::Code::NotFound | tonic::Code::FailedPrecondition | tonic::Code::Internal
        ),
        "{e}"
    );
}
