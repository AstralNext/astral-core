//! 在线 Agent 会话表（按 generation 安全下线；隧道 pending 按节点隔离）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::info;

use crate::error::{CoreError, CoreResult};
use crate::pb::{AgentSessionFrame, AgentTunnelRequest, NodeInfo};

/// 发往某个节点会话的出站帧发送端。
pub type FrameTx = mpsc::Sender<AgentSessionFrame>;

/// 已连接节点注册表。
#[derive(Clone, Default)]
pub struct SessionRegistry {
    inner: Arc<Mutex<HashMap<String, SessionEntry>>>,
    next_req_id: Arc<AtomicU64>,
    next_generation: Arc<AtomicU64>,
    pending: Arc<Mutex<HashMap<(String, u64), oneshot::Sender<crate::pb::AgentTunnelResponse>>>>,
}

struct SessionEntry {
    tx: FrameTx,
    info: NodeInfo,
    generation: u64,
}

impl SessionRegistry {
    /// 新建空表。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记节点会话，返回本会话 generation（下线时必须带上）。
    pub async fn insert(&self, node_id: String, info: NodeInfo, tx: FrameTx) -> u64 {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed) + 1;
        // 先清掉旧会话遗留的 pending，避免被新会话误用
        {
            let mut pending = self.pending.lock().await;
            pending.retain(|(nid, _), _| nid != &node_id);
        }
        let mut guard = self.inner.lock().await;
        guard.insert(
            node_id.clone(),
            SessionEntry {
                tx,
                info,
                generation,
            },
        );
        info!(%node_id, generation, "节点已上线");
        generation
    }

    /// 仅当 generation 仍匹配时移除（避免重连后旧任务误删新会话）。
    pub async fn remove(&self, node_id: &str, generation: u64) {
        let mut guard = self.inner.lock().await;
        let should_remove = guard
            .get(node_id)
            .map(|e| e.generation == generation)
            .unwrap_or(false);
        if should_remove {
            guard.remove(node_id);
            drop(guard);
            self.fail_pending_for(node_id).await;
            info!(%node_id, generation, "节点已离线");
        }
    }

    async fn fail_pending_for(&self, node_id: &str) {
        let mut pending = self.pending.lock().await;
        let keys: Vec<_> = pending
            .keys()
            .filter(|(nid, _)| nid == node_id)
            .cloned()
            .collect();
        for k in keys {
            if let Some(tx) = pending.remove(&k) {
                let _ = tx.send(crate::pb::AgentTunnelResponse {
                    id: k.1,
                    grpc_status: tonic::Code::Unavailable as i32,
                    message: "节点已离线".into(),
                    payload: vec![],
                });
            }
        }
    }

    /// 在线节点列表。
    pub async fn list_nodes(&self) -> Vec<NodeInfo> {
        let guard = self.inner.lock().await;
        guard
            .values()
            .map(|e| {
                let mut n = e.info.clone();
                n.online = true;
                n
            })
            .collect()
    }

    /// 查找节点信息。
    pub async fn get_node(&self, node_id: &str) -> Option<NodeInfo> {
        let guard = self.inner.lock().await;
        guard.get(node_id).map(|e| {
            let mut n = e.info.clone();
            n.online = true;
            n
        })
    }

    /// 经隧道代理 unary；超时返回错误。
    pub async fn proxy_unary(
        &self,
        node_id: &str,
        full_method: &str,
        payload: Vec<u8>,
        metadata: HashMap<String, String>,
    ) -> CoreResult<crate::pb::AgentTunnelResponse> {
        let tx = {
            let guard = self.inner.lock().await;
            guard
                .get(node_id)
                .map(|e| e.tx.clone())
                .ok_or_else(|| CoreError::NotFound(format!("节点未连接: {node_id}")))?
        };

        let id = self.next_req_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (resp_tx, resp_rx) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            pending.insert((node_id.to_string(), id), resp_tx);
        }

        let frame = AgentSessionFrame {
            body: Some(crate::pb::agent_session_frame::Body::TunnelRequest(
                AgentTunnelRequest {
                    id,
                    full_method: full_method.to_string(),
                    payload,
                    metadata,
                },
            )),
        };
        if tx.send(frame).await.is_err() {
            let mut pending = self.pending.lock().await;
            pending.remove(&(node_id.to_string(), id));
            return Err(CoreError::Internal("向节点发送隧道请求失败".into()));
        }

        match tokio::time::timeout(std::time::Duration::from_secs(60), resp_rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => Err(CoreError::Internal("隧道响应通道关闭".into())),
            Err(_) => {
                let mut pending = self.pending.lock().await;
                pending.remove(&(node_id.to_string(), id));
                Err(CoreError::Internal("等待节点隧道响应超时".into()))
            }
        }
    }

    /// 节点回传隧道响应时完成 pending（必须带 node_id，防跨节点伪造）。
    pub async fn complete_tunnel(&self, node_id: &str, resp: crate::pb::AgentTunnelResponse) {
        let mut pending = self.pending.lock().await;
        if let Some(tx) = pending.remove(&(node_id.to_string(), resp.id)) {
            let _ = tx.send(resp);
        }
    }
}
