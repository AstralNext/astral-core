//! 场景：InstanceService 生命周期与自启

use crate::harness::{
    config_source_toml, instance_ref, smoke_toml, try_start_smoke, TestServer,
};
use astral_core::pb::{
    DeleteInstanceRequest, GetInstanceConfigRequest, GetInstanceRequest, ListAutostartRequest,
    ListInstanceMetaRequest, ListInstancesRequest, RestartInstanceRequest, SetAutostartRequest,
    StopInstanceRequest, ValidateConfigRequest,
};
use tonic::Request;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s04_validate_good_and_bad() {
    let server = TestServer::start().await;
    let mut inst = server.instance().await;

    let bad = inst
        .validate_config(Request::new(ValidateConfigRequest {
            node: None,
            config: Some(config_source_toml("not = [valid".into())),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(!bad.valid);

    let good = inst
        .validate_config(Request::new(ValidateConfigRequest {
            node: None,
            config: Some(config_source_toml(smoke_toml())),
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(good.valid);
    assert_eq!(
        good.normalized_instance_id,
        "22222222-2222-4222-8222-222222222222"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s04_lifecycle_start_get_list_restart_stop_delete() {
    let server = TestServer::start().await;
    let mut inst = server.instance().await;
    let Some(started) = try_start_smoke(&mut inst).await else {
        return;
    };
    let id = started.instance_id.clone();

    let got = inst
        .get_instance(Request::new(GetInstanceRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await
        .expect("get")
        .into_inner();
    assert_eq!(got.summary.as_ref().unwrap().instance_id, id);

    let list = inst
        .list_instances(Request::new(ListInstancesRequest { node: None }))
        .await
        .unwrap()
        .into_inner();
    assert!(list.instances.iter().any(|i| i.instance_id == id));

    let meta = inst
        .list_instance_meta(Request::new(ListInstanceMetaRequest {
            node: None,
            instance_ids: vec![],
        }))
        .await
        .unwrap()
        .into_inner();
    assert!(meta.metas.iter().any(|m| m.instance_id == id));

    let cfg = inst
        .get_instance_config(Request::new(GetInstanceConfigRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await
        .expect("config")
        .into_inner();
    assert!(cfg.toml.contains("grpc-smoke"));

    let _ = inst
        .restart_instance(Request::new(RestartInstanceRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await
        .expect("restart");

    let _ = inst
        .stop_instance(Request::new(StopInstanceRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await
        .expect("stop");

    // Stop 幂等
    let _ = inst
        .stop_instance(Request::new(StopInstanceRequest {
            instance: Some(instance_ref(&id)),
        }))
        .await
        .expect("stop idempotent");

    let _ = inst
        .delete_instance(Request::new(DeleteInstanceRequest {
            instance: Some(instance_ref(&id)),
            clear_autostart: true,
        }))
        .await
        .expect("delete");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s04_autostart_set_and_list() {
    let server = TestServer::start().await;
    let mut inst = server.instance().await;

    let set = inst
        .set_autostart(Request::new(SetAutostartRequest {
            node: None,
            enabled: true,
            config: Some(config_source_toml(smoke_toml())),
            instance_id: String::new(),
            source_path: "scenario.toml".into(),
            start_now: false,
        }))
        .await
        .expect("set_autostart")
        .into_inner();
    assert_eq!(
        set.instance_id,
        "22222222-2222-4222-8222-222222222222"
    );

    let entries = inst
        .list_autostart(Request::new(ListAutostartRequest { node: None }))
        .await
        .unwrap()
        .into_inner()
        .entries;
    assert!(entries.iter().any(|e| e.instance_id == set.instance_id));

    let _ = inst
        .set_autostart(Request::new(SetAutostartRequest {
            node: None,
            enabled: false,
            config: None,
            instance_id: set.instance_id.clone(),
            source_path: String::new(),
            start_now: false,
        }))
        .await
        .expect("clear_autostart");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s04_profile_upsert_list_get_start_delete() {
    use astral_core::pb::{
        DeleteProfileRequest, GetProfileRequest, ListProfilesRequest, StartProfileRequest,
        UpsertProfileRequest,
    };

    let server = TestServer::start().await;
    let mut inst = server.instance().await;

    let up = inst
        .upsert_profile(Request::new(UpsertProfileRequest {
            node: None,
            config: Some(config_source_toml(smoke_toml())),
            display_name: "smoke-profile".into(),
            group: "test".into(),
            autostart: false,
        }))
        .await
        .expect("upsert")
        .into_inner();
    assert_eq!(
        up.instance_id,
        "22222222-2222-4222-8222-222222222222"
    );
    assert_eq!(up.summary.as_ref().unwrap().group, "test");

    let list = inst
        .list_profiles(Request::new(ListProfilesRequest {
            node: None,
            autostart_only: false,
        }))
        .await
        .unwrap()
        .into_inner()
        .profiles;
    assert!(list.iter().any(|p| p.instance_id == up.instance_id));

    let got = inst
        .get_profile(Request::new(GetProfileRequest {
            instance: Some(instance_ref(&up.instance_id)),
        }))
        .await
        .expect("get_profile")
        .into_inner();
    assert!(got.toml.contains("grpc-smoke"));

    let _ = inst
        .set_autostart(Request::new(SetAutostartRequest {
            node: None,
            enabled: true,
            config: None,
            instance_id: up.instance_id.clone(),
            source_path: String::new(),
            start_now: false,
        }))
        .await
        .expect("set_autostart by id");

    let auto_only = inst
        .list_profiles(Request::new(ListProfilesRequest {
            node: None,
            autostart_only: true,
        }))
        .await
        .unwrap()
        .into_inner()
        .profiles;
    assert!(auto_only.iter().any(|p| p.instance_id == up.instance_id));

    if try_start_smoke(&mut inst).await.is_some() {
        // already may be running from other path; prefer StartProfile
    }
    let _ = inst
        .start_profile(Request::new(StartProfileRequest {
            instance: Some(instance_ref(&up.instance_id)),
        }))
        .await;

    let _ = inst
        .delete_profile(Request::new(DeleteProfileRequest {
            instance: Some(instance_ref(&up.instance_id)),
            stop_if_running: true,
            clear_autostart: true,
        }))
        .await
        .expect("delete_profile");

    let after = inst
        .list_profiles(Request::new(ListProfilesRequest {
            node: None,
            autostart_only: false,
        }))
        .await
        .unwrap()
        .into_inner()
        .profiles;
    assert!(!after.iter().any(|p| p.instance_id == up.instance_id));
}
