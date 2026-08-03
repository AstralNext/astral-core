//! InstanceService：实例生命周期、配置库与自启。

use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::instance_service_server::InstanceService;
use crate::pb::{
    ConfigSource, DeleteInstanceRequest, DeleteInstanceResponse, DeleteProfileRequest,
    DeleteProfileResponse, GetInstanceConfigRequest, GetInstanceConfigResponse, GetInstanceRequest,
    GetInstanceResponse, GetProfileRequest, GetProfileResponse, InstanceState,
    ListAutostartRequest, ListAutostartResponse, ListInstanceMetaRequest, ListInstanceMetaResponse,
    ListInstancesRequest, ListInstancesResponse, ListProfilesRequest, ListProfilesResponse,
    RestartInstanceRequest, RestartInstanceResponse, RetainInstancesRequest,
    RetainInstancesResponse, SetAutostartRequest, SetAutostartResponse, StartInstanceRequest,
    StartInstanceResponse, StartProfileRequest, StartProfileResponse, StopInstanceRequest,
    StopInstanceResponse, UpsertProfileRequest, UpsertProfileResponse, ValidateConfigRequest,
    ValidateConfigResponse,
};
use crate::services::util::{
    ensure_local_node, parse_instance_id, require_instance_id, resolve_config, ResolvedConfig,
};

/// InstanceService 服务端。
pub struct InstanceSvc {
    state: AppState,
}

impl InstanceSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    fn upsert_profile_toml(
        &self,
        toml: &str,
        display_name: &str,
        group: &str,
        autostart: bool,
        preserve_autostart: bool,
    ) -> Result<crate::store::ProfileRecord, Status> {
        let id = self
            .state
            .engine
            .validate_toml(toml)
            .map_err(Status::from)?;
        let rec = self
            .state
            .profiles
            .upsert(
                &id.to_string(),
                toml,
                display_name,
                group,
                autostart,
                preserve_autostart,
            )
            .map_err(Status::from)?;
        self.state
            .engine
            .cache()
            .upsert(rec.to_cached())
            .map_err(Status::from)?;
        Ok(rec)
    }
}

#[tonic::async_trait]
impl InstanceService for InstanceSvc {
    async fn validate_config(
        &self,
        req: Request<ValidateConfigRequest>,
    ) -> Result<Response<ValidateConfigResponse>, Status> {
        let r = req.into_inner();
        ensure_local_node(&r.node, &self.state.node_id)?;
        let result = match resolve_config(&r.config)? {
            ResolvedConfig::Toml(t) => self.state.engine.validate_toml(&t),
            ResolvedConfig::Structured(s) => self.state.engine.validate_structured(&s),
        };
        match result {
            Ok(id) => Ok(Response::new(ValidateConfigResponse {
                valid: true,
                error_message: String::new(),
                normalized_instance_id: id.to_string(),
                warnings: vec![],
            })),
            Err(e) => Ok(Response::new(ValidateConfigResponse {
                valid: false,
                error_message: e.to_string(),
                normalized_instance_id: String::new(),
                warnings: vec![],
            })),
        }
    }

    async fn start_instance(
        &self,
        req: Request<StartInstanceRequest>,
    ) -> Result<Response<StartInstanceResponse>, Status> {
        let r = req.into_inner();
        ensure_local_node(&r.node, &self.state.node_id)?;
        let (id, toml_for_profile) = match resolve_config(&r.config)? {
            ResolvedConfig::Toml(t) => {
                let id = self
                    .state
                    .engine
                    .start_toml(&t, &r.display_name, &r.source_path)
                    .map_err(Status::from)?;
                (id, Some(t))
            }
            ResolvedConfig::Structured(s) => {
                let id = self
                    .state
                    .engine
                    .start_structured(&s, &r.display_name, &r.source_path)
                    .map_err(Status::from)?;
                let loader = crate::engine::structured_to_loader(&s).map_err(Status::from)?;
                let toml = crate::engine::loader_to_toml(&loader).map_err(Status::from)?;
                (id, Some(toml))
            }
        };
        if let Some(toml) = toml_for_profile {
            let _ = self.upsert_profile_toml(&toml, &r.display_name, "", false, true)?;
        }
        let mut summary = self.state.engine.summary_of(id).await;
        if !r.display_name.is_empty() {
            summary.display_name = r.display_name;
        }
        Ok(Response::new(StartInstanceResponse {
            instance_id: id.to_string(),
            state: summary.state,
            summary: Some(summary),
        }))
    }

    async fn stop_instance(
        &self,
        req: Request<StopInstanceRequest>,
    ) -> Result<Response<StopInstanceResponse>, Status> {
        let id_str = require_instance_id(&req.into_inner().instance)?;
        let id = parse_instance_id(&id_str)?;
        self.state.engine.stop(id).map_err(Status::from)?;
        Ok(Response::new(StopInstanceResponse {
            instance_id: id_str,
            state: InstanceState::Stopped as i32,
        }))
    }

    async fn restart_instance(
        &self,
        req: Request<RestartInstanceRequest>,
    ) -> Result<Response<RestartInstanceResponse>, Status> {
        let id_str = require_instance_id(&req.into_inner().instance)?;
        let id = parse_instance_id(&id_str)?;
        let new_id = self.state.engine.restart(id).map_err(Status::from)?;
        let summary = self.state.engine.summary_of(new_id).await;
        Ok(Response::new(RestartInstanceResponse {
            instance_id: new_id.to_string(),
            state: summary.state,
            summary: Some(summary),
        }))
    }

    async fn list_instances(
        &self,
        req: Request<ListInstancesRequest>,
    ) -> Result<Response<ListInstancesResponse>, Status> {
        ensure_local_node(&req.into_inner().node, &self.state.node_id)?;
        let instances = self.state.engine.list_summaries().await;
        Ok(Response::new(ListInstancesResponse { instances }))
    }

    async fn get_instance(
        &self,
        req: Request<GetInstanceRequest>,
    ) -> Result<Response<GetInstanceResponse>, Status> {
        let id_str = require_instance_id(&req.into_inner().instance)?;
        let id = parse_instance_id(&id_str)?;
        if !self.state.engine.exists(id)
            && self.state.engine.cache().get_uuid(id)?.is_none()
        {
            return Err(Status::not_found(format!("实例不存在: {id_str}")));
        }
        let summary = self.state.engine.summary_of(id).await;
        let cached = self.state.engine.cache().get_uuid(id)?;
        let autostart = self.state.profiles.is_autostart(&id_str)?;
        Ok(Response::new(GetInstanceResponse {
            summary: Some(summary),
            source_path: cached.map(|c| c.source_path).unwrap_or_default(),
            autostart,
            started_at: None,
        }))
    }

    async fn delete_instance(
        &self,
        req: Request<DeleteInstanceRequest>,
    ) -> Result<Response<DeleteInstanceResponse>, Status> {
        let r = req.into_inner();
        let id_str = require_instance_id(&r.instance)?;
        let id = parse_instance_id(&id_str)?;
        self.state.engine.delete(id).map_err(Status::from)?;
        if r.clear_autostart {
            if self.state.profiles.get(&id_str)?.is_some() {
                let _ = self.state.profiles.set_autostart(&id_str, false)?;
            }
            let _ = self.state.autostart.clear(&id_str);
        }
        Ok(Response::new(DeleteInstanceResponse {
            instance_id: id_str,
        }))
    }

    async fn retain_instances(
        &self,
        req: Request<RetainInstancesRequest>,
    ) -> Result<Response<RetainInstancesResponse>, Status> {
        let r = req.into_inner();
        ensure_local_node(&r.node, &self.state.node_id)?;
        let keep: std::collections::HashSet<String> = r.instance_ids.into_iter().collect();
        let mut kept = Vec::new();
        let mut removed = Vec::new();
        for id in self.state.engine.list_ids() {
            let s = id.to_string();
            if keep.contains(&s) {
                kept.push(s);
            } else {
                self.state.engine.delete(id).map_err(Status::from)?;
                removed.push(s);
            }
        }
        Ok(Response::new(RetainInstancesResponse {
            kept_ids: kept,
            removed_ids: removed,
        }))
    }

    async fn get_instance_config(
        &self,
        req: Request<GetInstanceConfigRequest>,
    ) -> Result<Response<GetInstanceConfigResponse>, Status> {
        let id_str = require_instance_id(&req.into_inner().instance)?;
        let id = parse_instance_id(&id_str)?;
        if let Some(prof) = self.state.profiles.get(&id_str)? {
            return Ok(Response::new(GetInstanceConfigResponse {
                toml: prof.toml,
                structured: None,
                source: ConfigSource::Unspecified as i32,
            }));
        }
        let cached = self
            .state
            .engine
            .cache()
            .get_uuid(id)?
            .ok_or_else(|| Status::not_found(format!("无缓存配置: {id_str}")))?;
        Ok(Response::new(GetInstanceConfigResponse {
            toml: cached.toml,
            structured: None,
            source: ConfigSource::Unspecified as i32,
        }))
    }

    async fn list_instance_meta(
        &self,
        req: Request<ListInstanceMetaRequest>,
    ) -> Result<Response<ListInstanceMetaResponse>, Status> {
        ensure_local_node(&req.get_ref().node, &self.state.node_id)?;
        let filter = &req.get_ref().instance_ids;
        let mut metas = Vec::new();
        for s in self.state.engine.list_summaries().await {
            if !filter.is_empty() && !filter.contains(&s.instance_id) {
                continue;
            }
            let cached = self.state.engine.cache().get(&s.instance_id)?;
            let autostart = self.state.profiles.is_autostart(&s.instance_id)?;
            metas.push(crate::pb::InstanceMeta {
                instance_id: s.instance_id,
                display_name: s.display_name,
                state: s.state,
                running: s.running,
                autostart,
                source_path: cached.map(|c| c.source_path).unwrap_or_default(),
                error_message: s.error_message,
            });
        }
        Ok(Response::new(ListInstanceMetaResponse { metas }))
    }

    async fn set_autostart(
        &self,
        req: Request<SetAutostartRequest>,
    ) -> Result<Response<SetAutostartResponse>, Status> {
        let r = req.into_inner();
        ensure_local_node(&r.node, &self.state.node_id)?;
        if r.enabled {
            let id = if r.config.is_some() {
                match resolve_config(&r.config)? {
                    ResolvedConfig::Toml(t) => {
                        let rec = self.upsert_profile_toml(&t, "", "", true, false)?;
                        rec.instance_id
                    }
                    ResolvedConfig::Structured(s) => {
                        let loader =
                            crate::engine::structured_to_loader(&s).map_err(Status::from)?;
                        let toml = crate::engine::loader_to_toml(&loader).map_err(Status::from)?;
                        let name = if s.hostname.is_empty() {
                            String::new()
                        } else {
                            s.hostname.clone()
                        };
                        let rec = self.upsert_profile_toml(&toml, &name, "", true, false)?;
                        rec.instance_id
                    }
                }
            } else if !r.instance_id.is_empty() {
                // 仅改标志；若无 profile 则尝试从运行缓存补齐
                if self.state.profiles.get(&r.instance_id)?.is_none() {
                    let cached = self
                        .state
                        .engine
                        .cache()
                        .get(&r.instance_id)?
                        .ok_or_else(|| {
                            Status::failed_precondition(
                                "enabled=true 需要已有 profile 或 config/缓存",
                            )
                        })?;
                    let _ = self.upsert_profile_toml(
                        &cached.toml,
                        &cached.display_name,
                        "",
                        true,
                        false,
                    )?;
                } else {
                    self.state
                        .profiles
                        .set_autostart(&r.instance_id, true)
                        .map_err(Status::from)?;
                }
                r.instance_id.clone()
            } else {
                return Err(Status::invalid_argument(
                    "enabled=true 需要 instance_id 或 config",
                ));
            };

            // 兼容：同步写一份到遗留 autostart（新逻辑以 profiles 为准）
            if let Ok(Some(prof)) = self.state.profiles.get(&id) {
                let _ = self.state.autostart.set(&prof.to_cached());
            }

            if r.start_now {
                if let Ok(Some(prof)) = self.state.profiles.get(&id) {
                    let _ = self
                        .state
                        .engine
                        .start_toml(&prof.toml, &prof.display_name, "")
                        .map_err(Status::from)?;
                }
            }
            Ok(Response::new(SetAutostartResponse { instance_id: id }))
        } else {
            let id = if !r.instance_id.is_empty() {
                r.instance_id
            } else {
                return Err(Status::invalid_argument(
                    "enabled=false 需要 instance_id",
                ));
            };
            if self.state.profiles.get(&id)?.is_some() {
                self.state
                    .profiles
                    .set_autostart(&id, false)
                    .map_err(Status::from)?;
            }
            let _ = self.state.autostart.clear(&id);
            Ok(Response::new(SetAutostartResponse { instance_id: id }))
        }
    }

    async fn list_autostart(
        &self,
        req: Request<ListAutostartRequest>,
    ) -> Result<Response<ListAutostartResponse>, Status> {
        ensure_local_node(&req.into_inner().node, &self.state.node_id)?;
        let entries = self.state.profiles.list_autostart_entries()?;
        Ok(Response::new(ListAutostartResponse { entries }))
    }

    async fn list_profiles(
        &self,
        req: Request<ListProfilesRequest>,
    ) -> Result<Response<ListProfilesResponse>, Status> {
        let r = req.into_inner();
        ensure_local_node(&r.node, &self.state.node_id)?;
        let profiles = self.state.profiles.list(r.autostart_only)?;
        Ok(Response::new(ListProfilesResponse { profiles }))
    }

    async fn get_profile(
        &self,
        req: Request<GetProfileRequest>,
    ) -> Result<Response<GetProfileResponse>, Status> {
        let id_str = require_instance_id(&req.into_inner().instance)?;
        let rec = self.state.profiles.require(&id_str)?;
        Ok(Response::new(GetProfileResponse {
            summary: Some(rec.to_summary()),
            toml: rec.toml,
        }))
    }

    async fn upsert_profile(
        &self,
        req: Request<UpsertProfileRequest>,
    ) -> Result<Response<UpsertProfileResponse>, Status> {
        let r = req.into_inner();
        ensure_local_node(&r.node, &self.state.node_id)?;
        let toml = match resolve_config(&r.config)? {
            ResolvedConfig::Toml(t) => t,
            ResolvedConfig::Structured(s) => {
                let loader = crate::engine::structured_to_loader(&s).map_err(Status::from)?;
                crate::engine::loader_to_toml(&loader).map_err(Status::from)?
            }
        };
        let rec = self.upsert_profile_toml(
            &toml,
            &r.display_name,
            &r.group,
            r.autostart,
            true, // 已存在保留原 autostart；新建用请求值
        )?;
        // UpsertProfile 的 autostart 字段：仅新建时生效（preserve=true 已处理）；
        // 若调用方明确想在已存在上改 autostart，应走 SetAutostart。
        Ok(Response::new(UpsertProfileResponse {
            instance_id: rec.instance_id.clone(),
            summary: Some(rec.to_summary()),
        }))
    }

    async fn delete_profile(
        &self,
        req: Request<DeleteProfileRequest>,
    ) -> Result<Response<DeleteProfileResponse>, Status> {
        let r = req.into_inner();
        let id_str = require_instance_id(&r.instance)?;
        let id = parse_instance_id(&id_str)?;
        if r.stop_if_running && self.state.engine.exists(id) {
            let _ = self.state.engine.delete(id);
        }
        if r.clear_autostart {
            let _ = self.state.autostart.clear(&id_str);
        }
        self.state.profiles.delete(&id_str)?;
        Ok(Response::new(DeleteProfileResponse {
            instance_id: id_str,
        }))
    }

    async fn start_profile(
        &self,
        req: Request<StartProfileRequest>,
    ) -> Result<Response<StartProfileResponse>, Status> {
        let id_str = require_instance_id(&req.into_inner().instance)?;
        let rec = self.state.profiles.require(&id_str)?;
        let id = self
            .state
            .engine
            .start_toml(&rec.toml, &rec.display_name, "")
            .map_err(Status::from)?;
        let summary = self.state.engine.summary_of(id).await;
        Ok(Response::new(StartProfileResponse {
            instance_id: id.to_string(),
            state: summary.state,
            summary: Some(summary),
        }))
    }
}
