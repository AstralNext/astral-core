//! 将 [`NetworkConfig`] 转为 EasyTier `TomlConfigLoader`。

use easytier::common::config::{
    ConfigLoader, NetworkIdentity, PeerConfig, PortForwardConfig, TomlConfigLoader, VpnPortalConfig,
};
use easytier::launcher::NetworkConfig as EtNetworkConfig;

use crate::error::{CoreError, CoreResult};
use crate::model::NetworkConfig;

/// 结构化配置 → EasyTier loader（优先走 ET `gen_config`，失败则手工映射）。
pub fn structured_to_loader(cfg: &NetworkConfig) -> CoreResult<TomlConfigLoader> {
    let et = astral_to_et_network_config(cfg);
    match et.gen_config() {
        Ok(loader) => Ok(loader),
        Err(e) => {
            tracing::warn!(error = %e, "ET gen_config 失败，回退手工映射");
            manual_loader(cfg)
        }
    }
}

/// astral → EasyTier manage.NetworkConfig。
pub fn astral_to_et_network_config(c: &NetworkConfig) -> EtNetworkConfig {
    EtNetworkConfig {
        instance_id: nonempty(&c.instance_id),
        dhcp: Some(c.dhcp),
        virtual_ipv4: nonempty(&c.ipv4),
        network_length: if c.network_length != 0 {
            Some(c.network_length)
        } else {
            None
        },
        hostname: nonempty(&c.hostname),
        network_name: nonempty(&c.network_name),
        network_secret: nonempty(&c.network_secret),
        networking_method: None,
        public_server_url: None,
        peer_urls: c.peers.clone(),
        proxy_cidrs: c.proxy_cidrs.clone(),
        enable_vpn_portal: Some(c.enable_vpn_portal),
        vpn_portal_listen_port: if c.vpn_portal_listen_port != 0 {
            Some(c.vpn_portal_listen_port)
        } else {
            None
        },
        vpn_portal_client_network_addr: nonempty(&c.vpn_portal_client_network),
        vpn_portal_client_network_len: if c.vpn_portal_client_network_len != 0 {
            Some(c.vpn_portal_client_network_len)
        } else {
            None
        },
        advanced_settings: Some(true),
        listener_urls: c.listeners.clone(),
        latency_first: Some(c.latency_first),
        dev_name: nonempty(&c.dev_name),
        use_smoltcp: Some(c.use_smoltcp),
        disable_ipv6: Some(!c.enable_ipv6),
        enable_kcp_proxy: Some(c.enable_kcp_proxy),
        disable_kcp_input: None,
        disable_p2p: Some(!c.enable_p2p),
        bind_device: Some(c.bind_device),
        no_tun: Some(!c.enable_tun),
        enable_exit_node: Some(c.enable_exit_node),
        relay_all_peer_rpc: Some(c.relay_all_peer_rpc),
        multi_thread: Some(c.multi_thread),
        enable_relay_network_whitelist: Some(!c.relay_network_whitelist.is_empty()),
        relay_network_whitelist: c.relay_network_whitelist.clone(),
        enable_manual_routes: Some(!c.routes.is_empty()),
        routes: c.routes.clone(),
        exit_nodes: c.exit_nodes.clone(),
        proxy_forward_by_system: Some(c.proxy_forward_by_system),
        disable_encryption: Some(!c.enable_encryption),
        enable_socks5: Some(c.enable_socks5),
        socks5_port: if c.socks5_port != 0 {
            Some(c.socks5_port)
        } else {
            None
        },
        disable_udp_hole_punching: Some(!c.enable_udp_hole_punching),
        mtu: if c.mtu != 0 { Some(c.mtu) } else { None },
        mapped_listeners: c.mapped_listeners.clone(),
        enable_magic_dns: Some(c.enable_magic_dns),
        enable_private_mode: Some(c.enable_private_mode),
        enable_quic_proxy: Some(c.enable_quic_proxy),
        disable_quic_input: None,
        #[allow(deprecated)]
        quic_listen_port: None,
        port_forwards: c
            .port_forwards
            .iter()
            .map(|p| easytier::proto::api::manage::PortForwardConfig {
                bind_ip: p.bind_ip.clone(),
                bind_port: p.bind_port,
                dst_ip: p.dst_ip.clone(),
                dst_port: p.dst_port,
                proto: p.protocol.clone(),
            })
            .collect(),
        disable_sym_hole_punching: None,
        p2p_only: None,
        data_compress_algo: None,
        encryption_algorithm: None,
        disable_tcp_hole_punching: None,
        secure_mode: Default::default(),
        acl: None,
        credential_file: None,
        lazy_p2p: None,
        need_p2p: None,
        instance_recv_bps_limit: None,
        disable_upnp: None,
        ipv6_public_addr_provider: None,
        ipv6_public_addr_auto: None,
        ipv6_public_addr_prefix: None,
        disable_relay_data: None,
        enable_udp_broadcast_relay: None,
    }
}

fn nonempty(s: &str) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

fn manual_loader(c: &NetworkConfig) -> CoreResult<TomlConfigLoader> {
    let cfg = TomlConfigLoader::default();
    if !c.instance_id.is_empty() {
        if let Ok(id) = uuid::Uuid::parse_str(&c.instance_id) {
            cfg.set_id(id);
        }
    }
    if !c.hostname.is_empty() {
        cfg.set_hostname(Some(c.hostname.clone()));
    }
    cfg.set_dhcp(c.dhcp);
    if !c.network_name.is_empty() {
        cfg.set_network_identity(NetworkIdentity::new(
            c.network_name.clone(),
            c.network_secret.clone(),
        ));
    }
    let mut listeners = Vec::new();
    for u in &c.listeners {
        listeners.push(
            u.parse()
                .map_err(|e| CoreError::InvalidArgument(format!("listener 无效 {u}: {e}")))?,
        );
    }
    if !listeners.is_empty() {
        cfg.set_listeners(listeners);
    }
    let mut peers = Vec::new();
    for u in &c.peers {
        peers.push(PeerConfig {
            uri: u
                .parse()
                .map_err(|e| CoreError::InvalidArgument(format!("peer 无效 {u}: {e}")))?,
            peer_public_key: None,
        });
    }
    cfg.set_peers(peers);
    for cidr in &c.proxy_cidrs {
        let parsed = cidr
            .parse()
            .map_err(|e| CoreError::InvalidArgument(format!("proxy_cidr 无效 {cidr}: {e}")))?;
        let _ = cfg.add_proxy_cidr(parsed, None);
    }
    if !c.dhcp && !c.ipv4.is_empty() {
        let ip = if c.ipv4.contains('/') {
            c.ipv4.clone()
        } else {
            format!("{}/{}", c.ipv4, if c.network_length > 0 { c.network_length } else { 24 })
        };
        cfg.set_ipv4(Some(
            ip.parse()
                .map_err(|e| CoreError::InvalidArgument(format!("ipv4 无效: {e}")))?,
        ));
    }
    let mut flags = cfg.get_flags();
    if !c.dev_name.is_empty() {
        flags.dev_name = c.dev_name.clone();
    }
    flags.enable_encryption = c.enable_encryption;
    flags.enable_ipv6 = c.enable_ipv6;
    if c.mtu > 0 {
        flags.mtu = c.mtu as u32;
    }
    flags.latency_first = c.latency_first;
    flags.enable_exit_node = c.enable_exit_node;
    flags.no_tun = !c.enable_tun;
    flags.use_smoltcp = c.use_smoltcp;
    flags.disable_p2p = !c.enable_p2p;
    flags.disable_udp_hole_punching = !c.enable_udp_hole_punching;
    flags.multi_thread = c.multi_thread;
    flags.bind_device = c.bind_device;
    flags.enable_kcp_proxy = c.enable_kcp_proxy;
    flags.enable_quic_proxy = c.enable_quic_proxy;
    flags.proxy_forward_by_system = c.proxy_forward_by_system;
    flags.accept_dns = c.enable_magic_dns;
    flags.private_mode = c.enable_private_mode;
    flags.relay_all_peer_rpc = c.relay_all_peer_rpc;
    if !c.relay_network_whitelist.is_empty() {
        flags.relay_network_whitelist = c.relay_network_whitelist.join(" ");
    }
    cfg.set_flags(flags);

    let mut forwards = Vec::new();
    for p in &c.port_forwards {
        let bind = format!("{}:{}", p.bind_ip, p.bind_port);
        let dst = format!("{}:{}", p.dst_ip, p.dst_port);
        forwards.push(PortForwardConfig {
            bind_addr: bind
                .parse()
                .map_err(|e| CoreError::InvalidArgument(format!("bind 无效: {e}")))?,
            dst_addr: dst
                .parse()
                .map_err(|e| CoreError::InvalidArgument(format!("dst 无效: {e}")))?,
            proto: p.protocol.clone(),
        });
    }
    if !forwards.is_empty() {
        cfg.set_port_forwards(forwards);
    }

    if c.enable_vpn_portal {
        let addr = if c.vpn_portal_client_network.is_empty() {
            "10.100.100.0".to_string()
        } else {
            c.vpn_portal_client_network.clone()
        };
        let len = if c.vpn_portal_client_network_len > 0 {
            c.vpn_portal_client_network_len
        } else {
            24
        };
        let cidr = format!("{addr}/{len}");
        let port = if c.vpn_portal_listen_port > 0 {
            c.vpn_portal_listen_port as u16
        } else {
            11013
        };
        cfg.set_vpn_portal_config(VpnPortalConfig {
            client_cidr: cidr
                .parse()
                .map_err(|e| CoreError::InvalidArgument(format!("vpn cidr 无效: {e}")))?,
            wireguard_listen: format!("0.0.0.0:{port}")
                .parse()
                .map_err(|e| CoreError::InvalidArgument(format!("vpn listen 无效: {e}")))?,
        });
    }

    Ok(cfg)
}

/// loader dump 为 TOML 字符串。
pub fn loader_to_toml(cfg: &TomlConfigLoader) -> CoreResult<String> {
    Ok(cfg.dump())
}
