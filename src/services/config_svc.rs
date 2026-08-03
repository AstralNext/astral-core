//! ConfigService：读配置、热补丁、整份替换。

use std::net::SocketAddr as StdSocketAddr;

use easytier::proto::api::config::{
    ConfigPatchAction, InstanceConfigPatch as EtPatch, PatchConfigRequest, PortForwardPatch,
};
use easytier::proto::common::{PortForwardConfigPb, SocketType};
use easytier::proto::rpc_types::controller::BaseController;
use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::config_service_server::ConfigService;
use crate::pb::{
    GetConfigRequest, GetConfigResponse, PatchConfigRequest as AstralPatchReq, PatchConfigResponse,
    ReplaceConfigRequest, ReplaceConfigResponse,
};
use crate::services::util::{
    parse_instance_id, require_instance_id, resolve_config, ResolvedConfig,
};
use crate::store::CachedInstance;

/// ConfigService 服务端。
pub struct ConfigSvc {
    state: AppState,
}

impl ConfigSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl ConfigService for ConfigSvc {
    async fn get_config(
        &self,
        req: Request<GetConfigRequest>,
    ) -> Result<Response<GetConfigResponse>, Status> {
        let id_str = require_instance_id(&req.into_inner().instance)?;
        let id = parse_instance_id(&id_str)?;
        let toml = self
            .state
            .engine
            .cache()
            .get_uuid(id)?
            .map(|c| c.toml)
            .unwrap_or_default();
        Ok(Response::new(GetConfigResponse {
            structured: None,
            toml,
            revision: String::new(),
        }))
    }

    async fn patch_config(
        &self,
        req: Request<AstralPatchReq>,
    ) -> Result<Response<PatchConfigResponse>, Status> {
        let r = req.into_inner();
        let id = parse_instance_id(&require_instance_id(&r.instance)?)?;
        let patch = r
            .patch
            .ok_or_else(|| Status::invalid_argument("缺少 patch"))?;

        let restart_required = patch.enable_vpn_portal.is_some()
            || !patch.vpn_portal_client_network.is_empty()
            || patch.vpn_portal_listen_port.is_some();

        if !patch.port_forwards.is_empty() {
            let mut et_pfs = Vec::new();
            for pf in &patch.port_forwards {
                let Some(entry) = &pf.entry else { continue };
                let bind: StdSocketAddr = format!("{}:{}", entry.bind_ip, entry.bind_port)
                    .parse()
                    .map_err(|e| Status::invalid_argument(format!("bind 无效: {e}")))?;
                let dst: StdSocketAddr = format!("{}:{}", entry.dst_ip, entry.dst_port)
                    .parse()
                    .map_err(|e| Status::invalid_argument(format!("dst 无效: {e}")))?;
                let socket_type = if entry.protocol.eq_ignore_ascii_case("udp") {
                    SocketType::Udp as i32
                } else {
                    SocketType::Tcp as i32
                };
                let action = match pf.action {
                    3 => ConfigPatchAction::Remove as i32,
                    4 => ConfigPatchAction::Clear as i32,
                    _ => ConfigPatchAction::Add as i32,
                };
                et_pfs.push(PortForwardPatch {
                    action,
                    cfg: Some(PortForwardConfigPb {
                        bind_addr: Some(bind.into()),
                        dst_addr: Some(dst.into()),
                        socket_type,
                    }),
                });
            }
            let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
            svc.get_config_service()
                .patch_config(
                    BaseController::default(),
                    PatchConfigRequest {
                        patch: Some(EtPatch {
                            port_forwards: et_pfs,
                            ..Default::default()
                        }),
                        instance: None,
                    },
                )
                .await
                .map_err(|e| Status::internal(e.to_string()))?;
        }

        if restart_required {
            let _ = self.state.engine.restart(id).map_err(Status::from)?;
        }

        Ok(Response::new(PatchConfigResponse {
            structured: None,
            revision: String::new(),
            restart_required,
        }))
    }

    async fn replace_config(
        &self,
        req: Request<ReplaceConfigRequest>,
    ) -> Result<Response<ReplaceConfigResponse>, Status> {
        let r = req.into_inner();
        let id_str = require_instance_id(&r.instance)?;
        let id = parse_instance_id(&id_str)?;
        let (toml, display) = match resolve_config(&r.config)? {
            ResolvedConfig::Toml(t) => (t, String::new()),
            ResolvedConfig::Structured(s) => {
                let loader = crate::engine::structured_to_loader(&s).map_err(Status::from)?;
                (
                    crate::engine::loader_to_toml(&loader).map_err(Status::from)?,
                    s.hostname,
                )
            }
        };
        let old = self.state.engine.cache().get_uuid(id)?;
        let rec = CachedInstance {
            instance_id: id_str.clone(),
            toml: toml.clone(),
            display_name: if display.is_empty() {
                old.as_ref()
                    .map(|o| o.display_name.clone())
                    .unwrap_or_default()
            } else {
                display
            },
            source_path: old.map(|o| o.source_path).unwrap_or_default(),
        };
        self.state.engine.cache().upsert(rec.clone())?;
        let restarted = if r.restart {
            let _ = self
                .state
                .engine
                .restart(id)
                .or_else(|_| {
                    self.state
                        .engine
                        .start_toml(&toml, &rec.display_name, &rec.source_path)
                })
                .map_err(Status::from)?;
            true
        } else {
            false
        };
        Ok(Response::new(ReplaceConfigResponse {
            restarted,
            instance_id: id_str,
            structured: None,
        }))
    }
}
