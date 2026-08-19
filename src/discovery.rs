//! 本机 UDP 服务发现：GUI 发 `ASTRAL_DISCOVER` → 内核回 `ASTRAL_CORE <addr>`。

use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tracing::{debug, warn};

/// 固定的 UDP 发现端口（与 RPC 端口 50051 相邻）。
pub const DISCOVERY_PORT: u16 = 50050;
const MAGIC: &[u8] = b"ASTRAL_DISCOVER";
const RESPONSE_PREFIX: &str = "ASTRAL_CORE ";

/// 在 `127.0.0.1:50050` 上监听发现请求，返回 RPC 地址。
/// 此函数不会返回（持续监听），应在 tokio::spawn 中运行。
pub async fn serve_discovery(rpc_addr: SocketAddr) {
    let bind: SocketAddr = ([127, 0, 0, 1], DISCOVERY_PORT).into();
    let socket = match UdpSocket::bind(bind).await {
        Ok(s) => s,
        Err(e) => {
            warn!(%bind, error = %e, "UDP 发现端口绑定失败，跳过发现服务");
            return;
        }
    };
    debug!(%bind, %rpc_addr, "UDP 服务发现已启动");

    let response = format!("{}{}", RESPONSE_PREFIX, rpc_addr);
    let resp_bytes = response.as_bytes();
    let mut buf = [0u8; 64];

    loop {
        let (len, peer) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "UDP recv_from 失败");
                continue;
            }
        };
        if len >= MAGIC.len() && &buf[..MAGIC.len()] == MAGIC {
            let _ = socket.send_to(resp_bytes, peer).await;
        }
    }
}

/// GUI 端：发送 UDP discover 请求，等待内核回复 RPC 地址。
/// 返回内核的 RPC 地址字符串（如 `127.0.0.1:50051`），超时返回 None。
pub fn parse_discovery_response(data: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(data).ok()?;
    s.strip_prefix(RESPONSE_PREFIX).map(|a| a.trim().to_string())
}
