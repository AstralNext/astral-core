//! 测试脚手架：临时 Core 进程内 JSON-RPC。

use std::time::Duration;

use astral_core::app::AppState;
use astral_core::config::RuntimeConfigBuilder;
use astral_core::rpc;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

/// 单测用服务器：独立 data-dir + 随机端口。
pub struct TestServer {
    _data: TempDir,
    pub addr: String,
    shutdown: Option<oneshot::Sender<()>>,
    join: Option<tokio::task::JoinHandle<()>>,
}

impl TestServer {
    pub async fn start() -> Self {
        let data = TempDir::new().expect("tempdir");
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let sock = listener.local_addr().expect("local_addr");
        let runtime = RuntimeConfigBuilder::new()
            .listen(sock)
            .data_dir(data.path().to_path_buf())
            .build()
            .expect("runtime");
        let state = AppState::bootstrap(runtime).expect("bootstrap");

        let (tx, rx) = oneshot::channel::<()>();
        let join = tokio::spawn(async move {
            let _ = rpc::serve_with_incoming_shutdown(state, listener, async move {
                let _ = rx.await;
            })
            .await;
        });

        let endpoint = sock.to_string();
        for _ in 0..80 {
            if rpc_call(&endpoint, "ping", json!({})).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

        Self {
            _data: data,
            addr: endpoint,
            shutdown: Some(tx),
            join: Some(join),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

/// CI 友好的无 TUN 配置（固定 instance_id）。
pub fn smoke_toml() -> String {
    r#"
instance_id = "22222222-2222-4222-8222-222222222222"
hostname = "rpc-smoke"
dhcp = true
listeners = ["udp://0.0.0.0:0"]
no_tun = true

[network_identity]
network_name = "rpc-smoke-net"
network_secret = "rpc-smoke-secret"
"#
    .to_string()
}

pub async fn rpc_call(addr: &str, method: &str, params: Value) -> Result<Value, (i64, String)> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    })
    .to_string();
    let req = format!(
        "POST / HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(|e| (-1, e.to_string()))?;
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| (-1, e.to_string()))?;
    let mut buf = Vec::new();
    stream
        .read_to_end(&mut buf)
        .await
        .map_err(|e| (-1, e.to_string()))?;
    let text = String::from_utf8_lossy(&buf);
    let body = text
        .split("\r\n\r\n")
        .nth(1)
        .ok_or_else(|| (-1, format!("无 HTTP body: {text}")))?;
    let v: Value = serde_json::from_str(body).map_err(|e| (-1, e.to_string()))?;
    if let Some(err) = v.get("error") {
        let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(0);
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        return Err((code, msg));
    }
    Ok(v.get("result").cloned().unwrap_or(Value::Null))
}

/// 启动冒烟实例；失败时返回 None（CI 无特权可接受）。
pub async fn try_start_smoke(server: &TestServer) -> Option<String> {
    match rpc_call(
        &server.addr,
        "instance.start",
        json!({
            "toml": smoke_toml(),
            "display_name": "rpc-smoke",
            "source_path": "",
        }),
    )
    .await
    {
        Ok(v) => v
            .get("instance_id")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string()),
        Err((_, e)) => {
            eprintln!("场景跳过 instance.start（环境限制）: {e}");
            None
        }
    }
}
