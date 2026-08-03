//! AclService：白名单与 ACL 统计 / 正文。

use easytier::proto::api::config::{
    AclPatch, ConfigPatchAction, InstanceConfigPatch, PatchConfigRequest, StringPatch,
};
use easytier::proto::api::instance::{GetAclStatsRequest, GetWhitelistRequest};
use easytier::proto::rpc_types::controller::BaseController;
use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::acl_service_server::AclService;
use crate::pb::{
    GetAclRequest, GetAclResponse, GetAclStatsRequest as AstralAclStatsReq, GetAclStatsResponse,
    GetWhitelistRequest as AstralWlReq, GetWhitelistResponse, PatchWhitelistRequest,
    PatchWhitelistResponse, SetAclRequest, SetAclResponse,
};
use crate::services::util::{et_patch_action, parse_instance_id, require_instance_id};

/// AclService 服务端。
pub struct AclSvc {
    state: AppState,
}

impl AclSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl AclService for AclSvc {
    async fn get_whitelist(
        &self,
        req: Request<AstralWlReq>,
    ) -> Result<Response<GetWhitelistResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_acl_manage_service()
            .get_whitelist(
                BaseController::default(),
                GetWhitelistRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetWhitelistResponse {
            tcp: resp.tcp_ports,
            udp: resp.udp_ports,
        }))
    }

    async fn patch_whitelist(
        &self,
        req: Request<PatchWhitelistRequest>,
    ) -> Result<Response<PatchWhitelistResponse>, Status> {
        let r = req.into_inner();
        let id = parse_instance_id(&require_instance_id(&r.instance)?)?;
        let map_patches = |patches: Vec<crate::pb::StringPatch>| {
            patches
                .into_iter()
                .map(|p| StringPatch {
                    action: et_patch_action(p.action),
                    value: p.value,
                })
                .collect::<Vec<_>>()
        };
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        svc.get_config_service()
            .patch_config(
                BaseController::default(),
                PatchConfigRequest {
                    patch: Some(InstanceConfigPatch {
                        acl: Some(AclPatch {
                            acl: None,
                            tcp_whitelist: map_patches(r.tcp),
                            udp_whitelist: map_patches(r.udp),
                        }),
                        ..Default::default()
                    }),
                    instance: None,
                },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let wl = svc
            .get_acl_manage_service()
            .get_whitelist(
                BaseController::default(),
                GetWhitelistRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(PatchWhitelistResponse {
            tcp: wl.tcp_ports,
            udp: wl.udp_ports,
        }))
    }

    async fn get_acl_stats(
        &self,
        req: Request<AstralAclStatsReq>,
    ) -> Result<Response<GetAclStatsResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_acl_manage_service()
            .get_acl_stats(
                BaseController::default(),
                GetAclStatsRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let raw = serde_json::to_string(&resp).unwrap_or_default();
        Ok(Response::new(GetAclStatsResponse {
            raw_json: raw,
            allow_hits: 0,
            deny_hits: 0,
        }))
    }

    async fn get_acl(
        &self,
        req: Request<GetAclRequest>,
    ) -> Result<Response<GetAclResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let cfg = svc
            .get_config_service()
            .get_config(
                BaseController::default(),
                easytier::proto::api::config::GetConfigRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let acl_json = cfg
            .config
            .and_then(|c| c.acl)
            .map(|a| serde_json::to_string(&a).unwrap_or_default())
            .unwrap_or_default();
        Ok(Response::new(GetAclResponse {
            acl_text: acl_json.clone(),
            format: "json".into(),
            raw_json: acl_json,
        }))
    }

    async fn set_acl(
        &self,
        req: Request<SetAclRequest>,
    ) -> Result<Response<SetAclResponse>, Status> {
        let r = req.into_inner();
        let id = parse_instance_id(&require_instance_id(&r.instance)?)?;
        let acl: easytier::proto::acl::Acl = serde_json::from_str(&r.acl_text)
            .map_err(|e| Status::invalid_argument(format!("ACL JSON 无效: {e}")))?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        svc.get_config_service()
            .patch_config(
                BaseController::default(),
                PatchConfigRequest {
                    patch: Some(InstanceConfigPatch {
                        acl: Some(AclPatch {
                            acl: Some(acl),
                            tcp_whitelist: vec![],
                            udp_whitelist: vec![],
                        }),
                        ..Default::default()
                    }),
                    instance: None,
                },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let _ = ConfigPatchAction::Add;
        Ok(Response::new(SetAclResponse {
            acl_text: r.acl_text,
            format: if r.format.is_empty() {
                "json".into()
            } else {
                r.format
            },
        }))
    }
}
