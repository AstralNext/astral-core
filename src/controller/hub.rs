//! 启动控制端 gRPC 监听。

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use crate::controller::admin_gate::AdminGateService;
use crate::controller::auth::ControllerAuth;
use crate::controller::hub_node::HubNodeSvc;
use crate::controller::proxy::AgentProxyService;
use crate::controller::sessions::SessionRegistry;
use crate::error::CoreResult;
use crate::pb::node_service_server::NodeServiceServer;
use crate::pb::system_service_server::SystemService;
use crate::pb::system_service_server::SystemServiceServer;
use crate::pb::{
    GetCapabilitiesRequest, GetCapabilitiesResponse, GetServerInfoRequest, GetServerInfoResponse,
    PingRequest, PingResponse, ServerMode,
};
use crate::tls_util::ServerTlsPaths;

/// 控制端运行参数。
#[derive(Debug, Clone)]
pub struct ControllerListenParams {
    /// 绑定地址。
    pub bind: SocketAddr,
    /// 与节点共享的 join / attestation 密钥。
    pub token: String,
    /// 控制端数据目录（设备凭证库）。
    pub data_dir: PathBuf,
    /// 可选 TLS（PEM 证书 + 私钥）；公网部署强烈建议启用。
    pub tls: Option<ServerTlsPaths>,
}

/// 运行控制端直到 shutdown。
pub async fn run_controller<F>(params: ControllerListenParams, shutdown: F) -> CoreResult<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let auth = Arc::new(ControllerAuth::open(&params.data_dir, params.token.clone())?);
    let sessions = SessionRegistry::new();
    let hub = HubNodeSvc::new(auth, sessions.clone());
    let sessions_for_layer = sessions.clone();
    let admin_token = params.token.clone();

    let tls_on = params.tls.is_some();
    if tls_on {
        info!(
            bind = %params.bind,
            data_dir = %params.data_dir.display(),
            "启动 astral-core 控制端（TLS）；管理 API 需 Bearer token"
        );
    } else {
        warn!(
            bind = %params.bind,
            data_dir = %params.data_dir.display(),
            "启动 astral-core 控制端（明文，仅建议本机/内网）；公网请加 --tls-cert/--tls-key"
        );
    }

    let mut builder = Server::builder();
    if let Some(tls) = &params.tls {
        builder = builder
            .tls_config(tls.load_server_config()?)
            .map_err(|e| crate::error::CoreError::Internal(format!("TLS 配置失败: {e}")))?;
    }

    builder
        .layer(tower::layer::layer_fn(move |inner| {
            let proxied = AgentProxyService::new(inner, sessions_for_layer.clone());
            AdminGateService::new(proxied, admin_token.clone())
        }))
        .add_service(SystemServiceServer::new(HubSystemSvc))
        .add_service(NodeServiceServer::new(hub))
        .serve_with_shutdown(params.bind, shutdown)
        .await
        .map_err(|e| crate::error::CoreError::Internal(format!("控制端服务失败: {e}")))?;
    Ok(())
}

struct HubSystemSvc;

#[tonic::async_trait]
impl SystemService for HubSystemSvc {
    async fn ping(&self, _req: Request<PingRequest>) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse { ok: true }))
    }

    async fn get_server_info(
        &self,
        _req: Request<GetServerInfoRequest>,
    ) -> Result<Response<GetServerInfoResponse>, Status> {
        Ok(Response::new(GetServerInfoResponse {
            api_version: "0.1.0".into(),
            core_version: env!("CARGO_PKG_VERSION").into(),
            protocol_package: "astral.v1".into(),
            mode: ServerMode::Control as i32,
            build_time: String::new(),
            git_commit: String::new(),
            capabilities: vec![
                "system.ping".into(),
                "controller.hub".into(),
                "agent.session".into(),
                "tunnel.proxy".into(),
            ],
        }))
    }

    async fn get_capabilities(
        &self,
        _req: Request<GetCapabilitiesRequest>,
    ) -> Result<Response<GetCapabilitiesResponse>, Status> {
        Ok(Response::new(GetCapabilitiesResponse {
            services: vec!["SystemService".into(), "NodeService".into()],
            features: vec!["agent-hub".into(), "tunnel-proxy".into()],
        }))
    }
}
