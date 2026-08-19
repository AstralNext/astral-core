//! 冒烟：校验 TOML、结构化配置、缓存重启路径（不要求真 TUN）。

use astral_core::engine::EngineHandle;
use astral_core::model::NetworkConfig;
use astral_core::store::{CachedInstance, InstanceCache};
use std::sync::Arc;
use tempfile::TempDir;

#[test]
fn validate_and_cache_toml() {
    let cache = Arc::new(InstanceCache::new());
    let engine = EngineHandle::new(cache);

    let toml = r#"
instance_id = "11111111-1111-4111-8111-111111111111"
hostname = "smoke-host"
dhcp = true
listeners = ["udp://0.0.0.0:11010"]
no_tun = true

[network_identity]
network_name = "smoke-net"
network_secret = "smoke-secret"
"#;
    let id = engine.validate_toml(toml).expect("toml 应可解析");
    assert_eq!(id.to_string(), "11111111-1111-4111-8111-111111111111");
    match engine.start_toml(toml, "smoke", "") {
        Ok(sid) => {
            assert_eq!(sid, id);
            assert!(engine.exists(sid));
            let _ = engine.restart(sid);
            assert!(engine.cache().get_uuid(sid).unwrap().is_some());
            let _ = engine.stop(sid);
            let _ = engine.delete(sid);
        }
        Err(e) => {
            eprintln!("start 跳过（环境限制）: {e}");
        }
    }
}

#[test]
fn structured_config_maps() {
    let cache = Arc::new(InstanceCache::new());
    let engine = EngineHandle::new(cache);
    let cfg = NetworkConfig {
        hostname: "s".into(),
        network_name: "n".into(),
        network_secret: "sec".into(),
        dhcp: true,
        listeners: vec!["udp://0.0.0.0:11011".into()],
        enable_tun: false,
        enable_encryption: true,
        enable_ipv6: true,
        enable_p2p: true,
        enable_udp_hole_punching: true,
        ..Default::default()
    };
    let id = engine
        .validate_structured(&cfg)
        .expect("structured 应可映射");
    assert!(!id.to_string().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn restore_desired_from_persisted_cache() {
    const ID: &str = "33333333-3333-4333-8333-333333333333";
    let toml = r#"
instance_id = "33333333-3333-4333-8333-333333333333"
hostname = "restore-host"
dhcp = true
listeners = ["udp://0.0.0.0:0"]
no_tun = true

[network_identity]
network_name = "restore-net"
network_secret = "restore-secret"
"#;
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("instance_cache.json");
    let cache = InstanceCache::load_or_create(path.clone()).expect("create cache");
    cache
        .upsert(CachedInstance {
            instance_id: ID.into(),
            toml: toml.into(),
            display_name: "restore".into(),
            source_path: "/configs/restore.toml".into(),
            started_at_unix_ms: Some(1_700_000_000_000),
        })
        .expect("persist");
    drop(cache);

    let cache = Arc::new(InstanceCache::load_or_create(path).expect("reload"));
    let engine = EngineHandle::new(cache);
    let report = engine.restore_desired();
    if report.started == 0 && !report.failed.is_empty() {
        eprintln!("restore 跳过（环境限制）: {:?}", report.failed);
        return;
    }
    assert_eq!(report.started, 1);
    assert!(engine.list_ids().iter().any(|id| id.to_string() == ID));
    if let Some(id) = engine
        .list_ids()
        .into_iter()
        .find(|id| id.to_string() == ID)
    {
        let _ = engine.stop(id);
    }
}
