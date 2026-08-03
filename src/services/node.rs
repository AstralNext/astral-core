//! NodeService：本机节点身份（单节点实现）。

use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::node_service_server::NodeService;
use crate::pb::{
    AgentHandshakeRequest, AgentHandshakeResponse, AgentHeartbeatRequest, AgentHeartbeatResponse,
    AgentSessionFrame, GenerateNodeEnrollTokenRequest, GenerateNodeEnrollTokenResponse,
    GetNodeHostInfoRequest, GetNodeHostInfoResponse, GetNodeRequest, GetNodeResponse,
    GetSelfNodeRequest, GetSelfNodeResponse, ListNodesRequest, ListNodesResponse, NodeInfo,
    RemoveNodeRequest, RemoveNodeResponse, RenameNodeRequest, RenameNodeResponse,
    RevokeNodeEnrollTokenRequest, RevokeNodeEnrollTokenResponse, SetNodeLabelsRequest,
    SetNodeLabelsResponse,
};
use crate::services::util::ensure_local_node;

/// NodeService 服务端。
pub struct NodeSvc {
    state: AppState,
}

impl NodeSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    fn self_info(&self) -> NodeInfo {
        NodeInfo {
            node_id: self.state.node_id.clone(),
            name: "local".into(),
            version: self.state.runtime.core_version.clone(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            online: true,
            labels: Default::default(),
            last_seen: None,
            capabilities: vec![
                "instance".into(),
                "network".into(),
                "credential".into(),
                "event".into(),
            ],
        }
    }
}

#[tonic::async_trait]
impl NodeService for NodeSvc {
    async fn get_self_node(
        &self,
        _req: Request<GetSelfNodeRequest>,
    ) -> Result<Response<GetSelfNodeResponse>, Status> {
        Ok(Response::new(GetSelfNodeResponse {
            node: Some(self.self_info()),
        }))
    }

    async fn list_nodes(
        &self,
        req: Request<ListNodesRequest>,
    ) -> Result<Response<ListNodesResponse>, Status> {
        let _ = req.into_inner().online_only;
        Ok(Response::new(ListNodesResponse {
            nodes: vec![self.self_info()],
        }))
    }

    async fn get_node(
        &self,
        req: Request<GetNodeRequest>,
    ) -> Result<Response<GetNodeResponse>, Status> {
        let id = req.into_inner().node_id;
        if id.is_empty() || id == self.state.node_id {
            Ok(Response::new(GetNodeResponse {
                node: Some(self.self_info()),
            }))
        } else {
            Err(Status::not_found(format!("节点不存在: {id}")))
        }
    }

    async fn rename_node(
        &self,
        _req: Request<RenameNodeRequest>,
    ) -> Result<Response<RenameNodeResponse>, Status> {
        Err(Status::unimplemented("RenameNode 尚未实现（需持久化节点名）"))
    }

    async fn set_node_labels(
        &self,
        _req: Request<SetNodeLabelsRequest>,
    ) -> Result<Response<SetNodeLabelsResponse>, Status> {
        Err(Status::unimplemented("SetNodeLabels 尚未实现"))
    }

    async fn remove_node(
        &self,
        _req: Request<RemoveNodeRequest>,
    ) -> Result<Response<RemoveNodeResponse>, Status> {
        Err(Status::unimplemented("RemoveNode 仅中控场景"))
    }

    async fn generate_node_enroll_token(
        &self,
        _req: Request<GenerateNodeEnrollTokenRequest>,
    ) -> Result<Response<GenerateNodeEnrollTokenResponse>, Status> {
        Err(Status::unimplemented("GenerateNodeEnrollToken 仅中控场景"))
    }

    async fn revoke_node_enroll_token(
        &self,
        _req: Request<RevokeNodeEnrollTokenRequest>,
    ) -> Result<Response<RevokeNodeEnrollTokenResponse>, Status> {
        Err(Status::unimplemented("RevokeNodeEnrollToken 仅中控场景"))
    }

    async fn agent_handshake(
        &self,
        _req: Request<AgentHandshakeRequest>,
    ) -> Result<Response<AgentHandshakeResponse>, Status> {
        Err(Status::unimplemented("AgentHandshake 仅中控场景"))
    }

    async fn agent_heartbeat(
        &self,
        _req: Request<AgentHeartbeatRequest>,
    ) -> Result<Response<AgentHeartbeatResponse>, Status> {
        Err(Status::unimplemented("AgentHeartbeat 仅中控场景"))
    }

    type AgentSessionStream =
        std::pin::Pin<Box<dyn tokio_stream::Stream<Item = Result<AgentSessionFrame, Status>> + Send>>;

    async fn agent_session(
        &self,
        _request: Request<tonic::Streaming<AgentSessionFrame>>,
    ) -> Result<Response<Self::AgentSessionStream>, Status> {
        Err(Status::unimplemented(
            "节点侧请使用出站 --controller；AgentSession 由中控提供",
        ))
    }

    async fn get_node_host_info(
        &self,
        req: Request<GetNodeHostInfoRequest>,
    ) -> Result<Response<GetNodeHostInfoResponse>, Status> {
        ensure_local_node(&req.into_inner().node, &self.state.node_id)?;
        Ok(Response::new(GetNodeHostInfoResponse {
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            memory_total_bytes: 0,
            memory_used_bytes: 0,
            cpu_usage_percent: 0.0,
            raw_json: String::new(),
        }))
    }
}
