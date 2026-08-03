//! SystemService 实现。

use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::system_service_server::SystemService;
use crate::pb::{
    GetCapabilitiesRequest, GetCapabilitiesResponse, GetServerInfoRequest, GetServerInfoResponse,
    PingRequest, PingResponse, ServerMode,
};

/// SystemService 服务端。
pub struct SystemSvc {
    state: AppState,
}

impl SystemSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl SystemService for SystemSvc {
    async fn ping(&self, _req: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse { ok: true }))
    }

    async fn get_server_info(
        &self,
        _req: Request<GetServerInfoRequest>,
    ) -> Result<Response<GetServerInfoResponse>, Status> {
        Ok(Response::new(GetServerInfoResponse {
            api_version: "0.1.0".into(),
            core_version: self.state.runtime.core_version.clone(),
            protocol_package: "astral.v1".into(),
            mode: ServerMode::Node as i32,
            build_time: String::new(),
            git_commit: String::new(),
            capabilities: vec![
                "system.ping".into(),
                "system.info".into(),
                "instance.validate".into(),
                "instance.start".into(),
                "instance.stop".into(),
                "instance.restart".into(),
                "instance.list".into(),
                "instance.get".into(),
                "instance.autostart".into(),
                "network.status".into(),
                "network.peers".into(),
                "network.routes".into(),
                "credential.token".into(),
                "node.self".into(),
                "event.subscribe".into(),
                "event.easytier_bus".into(),
                "config.patch".into(),
                "vpn.portal".into(),
                "portforward".into(),
                "acl".into(),
                "stats".into(),
                "app_message".into(),
            ],
        }))
    }

    async fn get_capabilities(
        &self,
        _req: Request<GetCapabilitiesRequest>,
    ) -> Result<Response<GetCapabilitiesResponse>, Status> {
        Ok(Response::new(GetCapabilitiesResponse {
            services: vec![
                "SystemService".into(),
                "InstanceService".into(),
                "NetworkService".into(),
                "CredentialService".into(),
                "NodeService".into(),
                "EventService".into(),
                "ConfigService".into(),
                "VpnPortalService".into(),
                "PortForwardService".into(),
                "AclService".into(),
                "StatsService".into(),
                "AppMessageService".into(),
                "LoggerService".into(),
                "BackupService".into(),
            ],
            features: vec![
                "auth.bearer_token".into(),
                "event.easytier_bus".into(),
                "autostart.persistent".into(),
            ],
        }))
    }
}
