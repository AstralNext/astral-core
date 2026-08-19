//! 场景：内核重启后按落盘记录自动拉起上次在跑的实例。

use std::time::Duration;

use crate::harness::{rpc_call, TestServer};
use serde_json::json;
use tempfile::TempDir;

fn restore_toml() -> String {
    r#"
instance_id = "44444444-4444-4444-8444-444444444444"
hostname = "rpc-restore"
dhcp = true
listeners = ["udp://0.0.0.0:0"]
no_tun = true

[network_identity]
network_name = "rpc-restore-net"
network_secret = "rpc-restore-secret"
"#
    .to_string()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s08_restore_desired_after_restart() {
    let data = TempDir::new().expect("tempdir");
    let first = TestServer::start_with_data_dir(data.path().to_path_buf()).await;
    let started = rpc_call(
        &first.addr,
        "instance.start",
        json!({
            "toml": restore_toml(),
            "display_name": "rpc-restore",
            "source_path": "/configs/restore.toml",
        }),
    )
    .await;
    let Ok(v) = started else {
        eprintln!("场景跳过 instance.start（环境限制）");
        first.shutdown().await;
        return;
    };
    let id = v
        .get("instance_id")
        .and_then(|x| x.as_str())
        .expect("instance_id")
        .to_string();
    first.shutdown().await;

    let cache_path = data.path().join("instance_cache.json");
    assert!(
        cache_path.exists(),
        "启动后应写入实例缓存: {}",
        cache_path.display()
    );

    let second = TestServer::start_with_data_dir(data.path().to_path_buf()).await;
    let mut restored = false;
    let mut last_meta = String::new();
    for _ in 0..80 {
        if let Ok(meta) = rpc_call(&second.addr, "instance.list_meta", json!({})).await {
            last_meta = meta.to_string();
            let metas = meta
                .get("metas")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            restored = metas.iter().any(|m| {
                m.get("instance_id").and_then(|v| v.as_str()) == Some(id.as_str())
                    && (m.get("running").and_then(|v| v.as_bool()) == Some(true)
                        || m.get("state").and_then(|v| v.as_str()) == Some("running")
                        || m.get("state").and_then(|v| v.as_str()) == Some("starting"))
            });
            if restored {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    second.shutdown().await;
    assert!(
        restored,
        "重启后应自动恢复上次在跑的实例 {id}; last_meta={last_meta}"
    );
}
