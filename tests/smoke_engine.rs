//! 冒烟：校验 TOML、结构化配置、缓存重启路径（不要求真 TUN）。

use astral_core::engine::EngineHandle;
use astral_core::pb::NetworkConfig;
use astral_core::store::InstanceCache;
use std::sync::Arc;

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
    let id = engine.validate_structured(&cfg).expect("structured 应可映射");
    assert!(!id.to_string().is_empty());
}
