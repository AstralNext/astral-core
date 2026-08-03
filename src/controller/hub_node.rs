//! 控制端 NodeService：AgentSession + 在线节点目录。

use std::pin::Pin;
use std::sync::Arc;

use futures_util::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status, Streaming};
use tracing::{info, warn};

use crate::controller::auth::ControllerAuth;
use crate::controller::sessions::SessionRegistry;
use crate::pb::node_service_server::NodeService;
use crate::pb::{
    agent_session_frame, AgentHandshakeRequest, AgentHandshakeResponse, AgentHeartbeatRequest,
    AgentHeartbeatResponse, AgentSessionFrame, GenerateNodeEnrollTokenRequest,
    GenerateNodeEnrollTokenResponse, GetNodeHostInfoRequest, GetNodeHostInfoResponse,
    GetNodeRequest, GetNodeResponse, GetSelfNodeRequest, GetSelfNodeResponse, ListNodesRequest,
    ListNodesResponse, NodeInfo, RemoveNodeRequest, RemoveNodeResponse, RenameNodeRequest,
    RenameNodeResponse, RevokeNodeEnrollTokenRequest, RevokeNodeEnrollTokenResponse,
    SetNodeLabelsRequest, SetNodeLabelsResponse,
};

/// 中控侧 NodeService。
pub struct HubNodeSvc {
    auth: Arc<ControllerAuth>,
    sessions: SessionRegistry,
}

impl HubNodeSvc {
    /// 创建。
    pub fn new(auth: Arc<ControllerAuth>, sessions: SessionRegistry) -> Self {
        Self { auth, sessions }
    }
}

type FrameStream = Pin<Box<dyn Stream<Item = Result<AgentSessionFrame, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl NodeService for HubNodeSvc {
    type AgentSessionStream = FrameStream;

    async fn get_self_node(
        &self,
        _req: Request<GetSelfNodeRequest>,
    ) -> Result<Response<GetSelfNodeResponse>, Status> {
        Ok(Response::new(GetSelfNodeResponse {
            node: Some(NodeInfo {
                node_id: "controller".into(),
                name: "astral-controller".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                os: std::env::consts::OS.into(),
                arch: std::env::consts::ARCH.into(),
                online: true,
                labels: Default::default(),
                last_seen: None,
                capabilities: vec!["controller".into(), "agent-hub".into()],
            }),
        }))
    }

    async fn list_nodes(
        &self,
        req: Request<ListNodesRequest>,
    ) -> Result<Response<ListNodesResponse>, Status> {
        let online_only = req.into_inner().online_only;
        let mut nodes = self.sessions.list_nodes().await;
        if online_only {
            nodes.retain(|n| n.online);
        }
        Ok(Response::new(ListNodesResponse { nodes }))
    }

    async fn get_node(
        &self,
        req: Request<GetNodeRequest>,
    ) -> Result<Response<GetNodeResponse>, Status> {
        let id = req.into_inner().node_id;
        match self.sessions.get_node(&id).await {
            Some(node) => Ok(Response::new(GetNodeResponse { node: Some(node) })),
            None => Err(Status::not_found(format!("节点未连接: {id}"))),
        }
    }

    async fn rename_node(
        &self,
        _req: Request<RenameNodeRequest>,
    ) -> Result<Response<RenameNodeResponse>, Status> {
        Err(Status::unimplemented("中控 RenameNode 尚未实现"))
    }

    async fn set_node_labels(
        &self,
        _req: Request<SetNodeLabelsRequest>,
    ) -> Result<Response<SetNodeLabelsResponse>, Status> {
        Err(Status::unimplemented("中控 SetNodeLabels 尚未实现"))
    }

    async fn remove_node(
        &self,
        _req: Request<RemoveNodeRequest>,
    ) -> Result<Response<RemoveNodeResponse>, Status> {
        Err(Status::unimplemented("RemoveNode 尚未实现"))
    }

    async fn generate_node_enroll_token(
        &self,
        _req: Request<GenerateNodeEnrollTokenRequest>,
    ) -> Result<Response<GenerateNodeEnrollTokenResponse>, Status> {
        Err(Status::unimplemented(
            "当前使用固定 --token 作为 join 密钥；动态加入码后续再做",
        ))
    }

    async fn revoke_node_enroll_token(
        &self,
        _req: Request<RevokeNodeEnrollTokenRequest>,
    ) -> Result<Response<RevokeNodeEnrollTokenResponse>, Status> {
        Err(Status::unimplemented("RevokeNodeEnrollToken 尚未实现"))
    }

    async fn agent_handshake(
        &self,
        req: Request<AgentHandshakeRequest>,
    ) -> Result<Response<AgentHandshakeResponse>, Status> {
        let inner = req.into_inner();
        match self.auth.authenticate(&inner) {
            Ok((node_id, device)) => Ok(Response::new(AgentHandshakeResponse {
                node_id,
                device_credential: device.unwrap_or_default(),
                session_hint: String::new(),
                controller_attestation: self.auth.attestation().to_string(),
            })),
            Err(e) => Err(Status::unauthenticated(e.to_string())),
        }
    }

    async fn agent_heartbeat(
        &self,
        _req: Request<AgentHeartbeatRequest>,
    ) -> Result<Response<AgentHeartbeatResponse>, Status> {
        Ok(Response::new(AgentHeartbeatResponse {
            ok: true,
            commands: vec![],
        }))
    }

    async fn agent_session(
        &self,
        request: Request<Streaming<AgentSessionFrame>>,
    ) -> Result<Response<Self::AgentSessionStream>, Status> {
        let mut inbound = request.into_inner();
        let auth = self.auth.clone();
        let sessions = self.sessions.clone();

        let (out_tx, out_rx) = mpsc::channel::<Result<AgentSessionFrame, Status>>(64);
        // 节点方向：控制端 → 节点（隧道请求等）
        let (node_tx, mut node_rx) = mpsc::channel::<AgentSessionFrame>(64);

        tokio::spawn(async move {
            // 首帧必须是 handshake
            let first = match inbound.next().await {
                Some(Ok(f)) => f,
                Some(Err(e)) => {
                    let _ = out_tx.send(Err(e)).await;
                    return;
                }
                None => {
                    let _ = out_tx
                        .send(Ok(AgentSessionFrame {
                            body: Some(agent_session_frame::Body::Error(
                                "空会话：需要 handshake 首帧".into(),
                            )),
                        }))
                        .await;
                    return;
                }
            };

            let hs = match first.body {
                Some(agent_session_frame::Body::Handshake(h)) => h,
                _ => {
                    let _ = out_tx
                        .send(Ok(AgentSessionFrame {
                            body: Some(agent_session_frame::Body::Error(
                                "首帧必须是 handshake".into(),
                            )),
                        }))
                        .await;
                    return;
                }
            };

            let (node_id, device) = match auth.authenticate(&hs) {
                Ok(v) => v,
                Err(e) => {
                    let _ = out_tx
                        .send(Ok(AgentSessionFrame {
                            body: Some(agent_session_frame::Body::Error(e.to_string())),
                        }))
                        .await;
                    return;
                }
            };

            let info = NodeInfo {
                node_id: node_id.clone(),
                name: if hs.name.is_empty() {
                    node_id.clone()
                } else {
                    hs.name
                },
                version: hs.core_version,
                os: hs.os,
                arch: hs.arch,
                online: true,
                labels: Default::default(),
                last_seen: None,
                capabilities: hs.capabilities,
            };

            let _ = out_tx
                .send(Ok(AgentSessionFrame {
                    body: Some(agent_session_frame::Body::HandshakeResult(
                        AgentHandshakeResponse {
                            node_id: node_id.clone(),
                            device_credential: device.unwrap_or_default(),
                            session_hint: String::new(),
                            controller_attestation: auth.attestation().to_string(),
                        },
                    )),
                }))
                .await;

            let generation = sessions
                .insert(node_id.clone(), info, node_tx.clone())
                .await;

            loop {
                tokio::select! {
                    // 控制端要发给节点的帧
                    frame = node_rx.recv() => {
                        match frame {
                            Some(f) => {
                                if out_tx.send(Ok(f)).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                    // 节点发来的帧
                    frame = inbound.next() => {
                        match frame {
                            Some(Ok(f)) => {
                                match f.body {
                                    Some(agent_session_frame::Body::Heartbeat(hb)) => {
                                        if !hb.node_id.is_empty() && hb.node_id != node_id {
                                            warn!(
                                                session = %node_id,
                                                claimed = %hb.node_id,
                                                "心跳 node_id 与会话不符，已忽略声明"
                                            );
                                        }
                                        info!(node_id = %node_id, "heartbeat");
                                        let _ = out_tx.send(Ok(AgentSessionFrame {
                                            body: Some(agent_session_frame::Body::HeartbeatResult(
                                                AgentHeartbeatResponse { ok: true, commands: vec![] },
                                            )),
                                        })).await;
                                    }
                                    Some(agent_session_frame::Body::TunnelResponse(resp)) => {
                                        sessions.complete_tunnel(&node_id, resp).await;
                                    }
                                    Some(agent_session_frame::Body::Error(msg)) => {
                                        warn!(%node_id, %msg, "节点报错");
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            Some(Err(e)) => {
                                warn!(%node_id, error = %e, "会话读错误");
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }

            sessions.remove(&node_id, generation).await;
        });

        let stream = ReceiverStream::new(out_rx);
        Ok(Response::new(Box::pin(stream) as Self::AgentSessionStream))
    }

    async fn get_node_host_info(
        &self,
        _req: Request<GetNodeHostInfoRequest>,
    ) -> Result<Response<GetNodeHostInfoResponse>, Status> {
        Err(Status::unimplemented("请带 x-astral-node-id 经隧道查询节点"))
    }
}
