//! VpnPortalService：查询与启停（启停通过改配置 + Restart）。

use easytier::proto::api::instance::GetVpnPortalInfoRequest as EtGetVpn;
use easytier::proto::rpc_types::controller::BaseController;
use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::vpn_portal_service_server::VpnPortalService;
use crate::pb::{
    DisableVpnPortalRequest, DisableVpnPortalResponse, EnableVpnPortalRequest,
    EnableVpnPortalResponse, GetVpnPortalInfoRequest, GetVpnPortalInfoResponse,
};
use crate::services::util::{parse_instance_id, require_instance_id};

/// VpnPortalService 服务端。
pub struct VpnSvc {
    state: AppState,
}

impl VpnSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn fetch_info(&self, id: uuid::Uuid) -> Result<GetVpnPortalInfoResponse, Status> {
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_vpn_portal_service()
            .get_vpn_portal_info(BaseController::default(), EtGetVpn { instance: None })
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let info = resp.vpn_portal_info.unwrap_or_default();
        let raw = serde_json::to_string(&info).unwrap_or_default();
        let enabled = !info.client_config.is_empty() || !info.vpn_type.is_empty();
        Ok(GetVpnPortalInfoResponse {
            enabled,
            listen_port: 0,
            client_network: String::new(),
            client_network_len: 0,
            hint_text: info.client_config.clone(),
            raw_json: raw,
        })
    }
}

#[tonic::async_trait]
impl VpnPortalService for VpnSvc {
    async fn get_vpn_portal_info(
        &self,
        req: Request<GetVpnPortalInfoRequest>,
    ) -> Result<Response<GetVpnPortalInfoResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        Ok(Response::new(self.fetch_info(id).await?))
    }

    async fn enable_vpn_portal(
        &self,
        req: Request<EnableVpnPortalRequest>,
    ) -> Result<Response<EnableVpnPortalResponse>, Status> {
        let r = req.into_inner();
        let id_str = require_instance_id(&r.instance)?;
        let id = parse_instance_id(&id_str)?;
        let mut cached = self
            .state
            .engine
            .cache()
            .get_uuid(id)?
            .ok_or_else(|| Status::failed_precondition("无缓存配置，无法启用 VPN Portal"))?;
        // TOML 级开关：追加/替换 vpn_portal 段（简化：写入标记后依赖用户配置；此处用 restart + 提示）
        // 更稳妥：用 structured 路径。先在 toml 末尾附加注释性配置块若缺失。
        if !cached.toml.contains("vpn_portal") && !cached.toml.contains("enable_vpn_portal") {
            let port = if r.listen_port > 0 {
                r.listen_port
            } else {
                11013
            };
            let net = if r.client_network.is_empty() {
                "10.100.100.0".into()
            } else {
                r.client_network
            };
            let len = if r.client_network_len > 0 {
                r.client_network_len
            } else {
                24
            };
            cached.toml.push_str(&format!(
                "\n[vpn_portal]\nwireguard_listen = \"0.0.0.0:{port}\"\nclient_cidr = \"{net}/{len}\"\n"
            ));
            self.state.engine.cache().upsert(cached)?;
        }
        let new_id = self.state.engine.restart(id).map_err(Status::from)?;
        let info = self.fetch_info(new_id).await?;
        Ok(Response::new(EnableVpnPortalResponse { info: Some(info) }))
    }

    async fn disable_vpn_portal(
        &self,
        req: Request<DisableVpnPortalRequest>,
    ) -> Result<Response<DisableVpnPortalResponse>, Status> {
        let id_str = require_instance_id(&req.into_inner().instance)?;
        let id = parse_instance_id(&id_str)?;
        // 简化：重启前从缓存删除 [vpn_portal] 段较复杂；标记 restart 即可让用户改配置。
        // 这里尝试去掉包含 vpn_portal 的行块（粗粒度）。
        if let Some(mut cached) = self.state.engine.cache().get_uuid(id)? {
            if let Some(pos) = cached.toml.find("[vpn_portal]") {
                cached.toml.truncate(pos);
                self.state.engine.cache().upsert(cached)?;
            }
            let _ = self.state.engine.restart(id).map_err(Status::from)?;
        }
        Ok(Response::new(DisableVpnPortalResponse {}))
    }
}
