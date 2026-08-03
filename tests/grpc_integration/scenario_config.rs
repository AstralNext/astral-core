//! 场景：ConfigService

use crate::harness::{
    config_source_toml, instance_ref, smoke_toml, try_start_smoke, TestServer,
};
use astral_core::pb::{
    GetConfigRequest, ReplaceConfigRequest, StopInstanceRequest,
};
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s08_get_and_replace_config() {
    let server = TestServer::start().await;
    let mut inst = server.instance().await;
    let Some(started) = try_start_smoke(&mut inst).await else {
        return;
    };
    let id = started.instance_id.clone();
    let mut cfg = server.config().await;

    let got = cfg
        .get_config(Request::new(GetConfigRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await
        .expect("get")
        .into_inner();
    assert!(got.toml.contains("grpc-smoke"));

    let mut new_toml = smoke_toml();
    new_toml.push_str("\n# scenario-replace\n");
    let replaced = cfg
        .replace_config(Request::new(ReplaceConfigRequest {
            instance: Some(instance_ref(&id)),
            config: Some(config_source_toml(new_toml)),
            restart: false,
        }))
        .await
        .expect("replace")
        .into_inner();
    assert_eq!(replaced.instance_id, id);
    assert!(!replaced.restarted);

    let got2 = cfg
        .get_config(Request::new(GetConfigRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(got2.toml.contains("scenario-replace"));

    let _ = inst
        .stop_instance(Request::new(StopInstanceRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await;
}
