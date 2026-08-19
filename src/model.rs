//! GUI JSON-RPC 用到的数据结构。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// 实例运行态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceState {
    /// 未指定。
    Unspecified,
    /// 已停止。
    Stopped,
    /// 启动中。
    Starting,
    /// 运行中。
    Running,
    /// 停止中。
    Stopping,
    /// 出错。
    Error,
}

/// 实例摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceSummary {
    /// 实例 ID。
    pub instance_id: String,
    /// UI 显示名。
    pub display_name: String,
    /// 运行态。
    pub state: InstanceState,
    /// 是否可视为在跑。
    pub running: bool,
    /// 错误说明。
    pub error_message: String,
    /// 虚拟网卡名。
    pub dev_name: String,
    /// 网络名。
    pub network_name: String,
    /// 主机名。
    pub hostname: String,
    /// 本次运行开始时刻（Unix 毫秒）；未运行时为 null。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_unix_ms: Option<u64>,
}

/// 列表用轻量元数据。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceMeta {
    /// 实例 ID。
    pub instance_id: String,
    /// UI 显示名。
    pub display_name: String,
    /// 运行态。
    pub state: InstanceState,
    /// 是否可视为在跑。
    pub running: bool,
    /// 配置来源路径。
    pub source_path: String,
    /// 错误说明。
    pub error_message: String,
    /// 本次运行开始时刻（Unix 毫秒）；未运行时为 null。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at_unix_ms: Option<u64>,
}

/// Peer 摘要。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerSummary {
    /// Peer ID。
    pub peer_id: String,
    /// 主机名。
    pub hostname: String,
    /// IPv4。
    pub ipv4: String,
    /// IPv6。
    pub ipv6: String,
    /// 延迟毫秒；负值表示未知。
    pub latency_ms: f64,
    /// 丢包百分比。
    pub loss_percent: f64,
    /// local / p2p / relay。
    pub conn_type: String,
    /// 收字节。
    pub rx_bytes: u64,
    /// 发字节。
    pub tx_bytes: u64,
}

/// 端口转发条目（结构化配置内嵌）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PortForwardEntry {
    /// tcp / udp。
    #[serde(default)]
    pub protocol: String,
    /// 绑定 IP。
    #[serde(default)]
    pub bind_ip: String,
    /// 绑定端口。
    #[serde(default)]
    pub bind_port: u32,
    /// 目标 IP。
    #[serde(default)]
    pub dst_ip: String,
    /// 目标端口。
    #[serde(default)]
    pub dst_port: u32,
}

/// 结构化网络配置（引擎内部仍支持；JSON-RPC 启停只走 TOML）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// 实例 ID。
    #[serde(default)]
    pub instance_id: String,
    /// 主机名。
    #[serde(default)]
    pub hostname: String,
    /// 网络名。
    #[serde(default)]
    pub network_name: String,
    /// 网络密钥。
    #[serde(default)]
    pub network_secret: String,
    /// DHCP。
    #[serde(default)]
    pub dhcp: bool,
    /// 虚拟 IPv4。
    #[serde(default)]
    pub ipv4: String,
    /// 前缀长度。
    #[serde(default)]
    pub network_length: i32,
    /// 虚拟 IPv6。
    #[serde(default)]
    pub ipv6: String,
    /// 监听 URL。
    #[serde(default)]
    pub listeners: Vec<String>,
    /// 对端 URL。
    #[serde(default)]
    pub peers: Vec<String>,
    /// mapped listeners。
    #[serde(default)]
    pub mapped_listeners: Vec<String>,
    /// 代理网段。
    #[serde(default)]
    pub proxy_cidrs: Vec<String>,
    /// 静态路由。
    #[serde(default)]
    pub routes: Vec<String>,
    /// 出口节点。
    #[serde(default)]
    pub exit_nodes: Vec<String>,
    /// VPN Portal。
    #[serde(default)]
    pub enable_vpn_portal: bool,
    /// VPN 监听端口。
    #[serde(default)]
    pub vpn_portal_listen_port: i32,
    /// VPN 客户端网段。
    #[serde(default)]
    pub vpn_portal_client_network: String,
    /// VPN 客户端前缀。
    #[serde(default)]
    pub vpn_portal_client_network_len: i32,
    /// 网卡名。
    #[serde(default)]
    pub dev_name: String,
    /// MTU。
    #[serde(default)]
    pub mtu: i32,
    /// 延迟优先。
    #[serde(default)]
    pub latency_first: bool,
    /// 多线程。
    #[serde(default)]
    pub multi_thread: bool,
    /// 加密。
    #[serde(default)]
    pub enable_encryption: bool,
    /// IPv6。
    #[serde(default)]
    pub enable_ipv6: bool,
    /// KCP 代理。
    #[serde(default)]
    pub enable_kcp_proxy: bool,
    /// QUIC 代理。
    #[serde(default)]
    pub enable_quic_proxy: bool,
    /// P2P。
    #[serde(default)]
    pub enable_p2p: bool,
    /// UDP 打洞。
    #[serde(default)]
    pub enable_udp_hole_punching: bool,
    /// TUN。
    #[serde(default)]
    pub enable_tun: bool,
    /// smoltcp。
    #[serde(default)]
    pub use_smoltcp: bool,
    /// Magic DNS。
    #[serde(default)]
    pub enable_magic_dns: bool,
    /// 私有模式。
    #[serde(default)]
    pub enable_private_mode: bool,
    /// 出口节点。
    #[serde(default)]
    pub enable_exit_node: bool,
    /// SOCKS5。
    #[serde(default)]
    pub enable_socks5: bool,
    /// SOCKS5 端口。
    #[serde(default)]
    pub socks5_port: i32,
    /// 绑定网卡。
    #[serde(default)]
    pub bind_device: bool,
    /// 系统转发。
    #[serde(default)]
    pub proxy_forward_by_system: bool,
    /// 中继全部 peer RPC。
    #[serde(default)]
    pub relay_all_peer_rpc: bool,
    /// 中继白名单。
    #[serde(default)]
    pub relay_network_whitelist: Vec<String>,
    /// 端口转发。
    #[serde(default)]
    pub port_forwards: Vec<PortForwardEntry>,
    /// 逃逸舱。
    #[serde(default)]
    pub extra: HashMap<String, String>,
}
