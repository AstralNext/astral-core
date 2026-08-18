//! 从 EasyTier 运行信息映射 Peer / 网络状态。

use easytier::launcher::NetworkInstanceRunningInfo;
use easytier::proto::api::instance::{list_peer_route_pair, PeerRoutePair};

use crate::model::PeerSummary;

/// 将引擎运行快照转为 Peer 摘要列表；始终包含本机合成节点。
pub fn peer_summaries_from_info(info: &NetworkInstanceRunningInfo) -> Vec<PeerSummary> {
    let pairs = peer_route_pairs_from_info(info);
    let mut peers: Vec<PeerSummary> = pairs
        .into_iter()
        .filter_map(|p| pair_to_summary(&p))
        .collect();
    merge_local_peer(
        &mut peers,
        &hostname_from_info(info),
        &my_ipv4_from_info(info),
    );
    peers
}

/// 是否已有本机节点。
pub fn has_local_peer(peers: &[PeerSummary]) -> bool {
    peers.iter().any(|p| {
        p.conn_type.eq_ignore_ascii_case("local") || p.peer_id == "0"
    })
}

/// 在列表头部补上本机（无对端时也应能看见自己）。
pub fn merge_local_peer(peers: &mut Vec<PeerSummary>, hostname: &str, ipv4: &str) {
    if has_local_peer(peers) {
        return;
    }
    if !ipv4.is_empty() && peers.iter().any(|p| p.ipv4 == ipv4) {
        return;
    }
    let hostname = if hostname.trim().is_empty() {
        "local".to_string()
    } else {
        hostname.to_string()
    };
    peers.insert(
        0,
        PeerSummary {
            peer_id: "0".into(),
            hostname,
            ipv4: ipv4.to_string(),
            ipv6: String::new(),
            latency_ms: 0.0,
            loss_percent: 0.0,
            conn_type: "local".into(),
            rx_bytes: 0,
            tx_bytes: 0,
        },
    );
}

/// 组装 peer/route 对（与旧 FRB 适配层逻辑对齐）。
pub fn peer_route_pairs_from_info(info: &NetworkInstanceRunningInfo) -> Vec<PeerRoutePair> {
    let mut pairs = if info.peer_route_pairs.is_empty() {
        list_peer_route_pair(info.peers.clone(), info.routes.clone())
    } else {
        info.peer_route_pairs.clone()
    };

    let mut seen: std::collections::HashSet<u32> = pairs
        .iter()
        .filter_map(|p| p.route.as_ref().map(|r| r.peer_id))
        .collect();

    for peer in &info.peers {
        if !seen.contains(&peer.peer_id) {
            pairs.push(PeerRoutePair {
                route: None,
                peer: Some(peer.clone()),
            });
            seen.insert(peer.peer_id);
        }
    }
    pairs
}

fn pair_to_summary(pair: &PeerRoutePair) -> Option<PeerSummary> {
    let route = pair.route.as_ref()?;
    let peer_id = route.peer_id.to_string();
    let hostname = route.hostname.clone();
    let ipv4 = route
        .ipv4_addr
        .as_ref()
        .map(|a| a.to_string())
        .unwrap_or_default();
    let ipv6 = route
        .ipv6_addr
        .as_ref()
        .map(|a| a.to_string())
        .unwrap_or_default();

    let latency_ms = if route.cost == 1 {
        pair.get_latency_ms().unwrap_or(-1.0)
    } else if route.cost == 0 {
        0.0
    } else {
        -1.0
    };

    let conn_type = match route.cost {
        0 => "local",
        1 => "p2p",
        _ => "relay",
    }
    .to_string();

    let (rx_bytes, tx_bytes) = pair
        .peer
        .as_ref()
        .map(|p| {
            let mut rx = 0u64;
            let mut tx = 0u64;
            for c in &p.conns {
                rx = rx.saturating_add(c.stats.as_ref().map(|s| s.rx_bytes).unwrap_or(0));
                tx = tx.saturating_add(c.stats.as_ref().map(|s| s.tx_bytes).unwrap_or(0));
            }
            (rx, tx)
        })
        .unwrap_or((0, 0));

    Some(PeerSummary {
        peer_id,
        hostname,
        ipv4,
        ipv6,
        latency_ms,
        loss_percent: -1.0,
        conn_type,
        rx_bytes,
        tx_bytes,
    })
}

/// 从 my_node_info 提取本机虚拟 IPv4 字符串。
pub fn my_ipv4_from_info(info: &NetworkInstanceRunningInfo) -> String {
    info.my_node_info
        .as_ref()
        .and_then(|n| n.virtual_ipv4.as_ref())
        .map(|a| a.to_string())
        .unwrap_or_default()
}

/// 本机 hostname。
pub fn hostname_from_info(info: &NetworkInstanceRunningInfo) -> String {
    info.my_node_info
        .as_ref()
        .map(|n| n.hostname.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(id: &str, host: &str, ipv4: &str, conn: &str) -> PeerSummary {
        PeerSummary {
            peer_id: id.into(),
            hostname: host.into(),
            ipv4: ipv4.into(),
            ipv6: String::new(),
            latency_ms: 1.0,
            loss_percent: 0.0,
            conn_type: conn.into(),
            rx_bytes: 0,
            tx_bytes: 0,
        }
    }

    #[test]
    fn merge_local_on_empty_list() {
        let mut peers = Vec::new();
        merge_local_peer(&mut peers, "alpha", "10.126.126.1");
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].peer_id, "0");
        assert_eq!(peers[0].conn_type, "local");
        assert_eq!(peers[0].hostname, "alpha");
        assert_eq!(peers[0].ipv4, "10.126.126.1");
    }

    #[test]
    fn merge_local_skips_when_already_local() {
        let mut peers = vec![summary("0", "alpha", "10.126.126.1", "local")];
        merge_local_peer(&mut peers, "alpha", "10.126.126.1");
        assert_eq!(peers.len(), 1);
    }

    #[test]
    fn merge_local_skips_same_ipv4() {
        let mut peers = vec![summary("9", "alpha", "10.126.126.1", "p2p")];
        merge_local_peer(&mut peers, "alpha", "10.126.126.1");
        assert_eq!(peers.len(), 1);
    }
}
