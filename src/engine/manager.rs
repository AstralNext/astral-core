//! 对 EasyTier [`NetworkInstanceManager`] 的封装。

use std::sync::Arc;

use easytier::common::config::{ConfigFileControl, ConfigLoader, TomlConfigLoader};
use easytier::instance_manager::NetworkInstanceManager;
use easytier::launcher::NetworkInstanceRunningInfo;
use uuid::Uuid;

use crate::engine::peers::{
    has_local_peer, hostname_from_info, merge_local_peer, my_ipv4_from_info, peer_summaries_from_info,
};
use crate::engine::structured::{loader_to_toml, structured_to_loader};
use crate::error::{CoreError, CoreResult};
use crate::model::{InstanceState, InstanceSummary, NetworkConfig, PeerSummary};
use crate::store::{CachedInstance, InstanceCache};

/// 引擎句柄：进程内唯一实例管理器 + 配置缓存钩子。
#[derive(Clone)]
pub struct EngineHandle {
    manager: Arc<NetworkInstanceManager>,
    cache: Arc<InstanceCache>,
}

impl EngineHandle {
    /// 创建。
    pub fn new(cache: Arc<InstanceCache>) -> Self {
        Self {
            manager: Arc::new(NetworkInstanceManager::new()),
            cache,
        }
    }

    /// 底层管理器。
    pub fn manager(&self) -> &Arc<NetworkInstanceManager> {
        &self.manager
    }

    /// 配置缓存。
    pub fn cache(&self) -> &Arc<InstanceCache> {
        &self.cache
    }

    /// 注入 TUN fd（移动端 / 自定义 TUN）。
    pub fn set_tun_fd(&self, id: Uuid, fd: i32) -> CoreResult<()> {
        self.manager
            .set_tun_fd(&id, fd)
            .map_err(|e| CoreError::Internal(format!("set_tun_fd 失败: {e:#}")))
    }

    /// 校验 TOML。
    pub fn validate_toml(&self, toml: &str) -> CoreResult<Uuid> {
        let cfg = TomlConfigLoader::new_from_str(toml)
            .map_err(|e| CoreError::FailedPrecondition(format!("配置 TOML 无效: {e:#}")))?;
        Ok(cfg.get_id())
    }

    /// 校验结构化配置。
    pub fn validate_structured(&self, cfg: &NetworkConfig) -> CoreResult<Uuid> {
        Ok(structured_to_loader(cfg)?.get_id())
    }

    /// 用 TOML 启动并写入缓存。
    pub fn start_toml(
        &self,
        toml: &str,
        display_name: &str,
        source_path: &str,
    ) -> CoreResult<Uuid> {
        let cfg = TomlConfigLoader::new_from_str(toml)
            .map_err(|e| CoreError::FailedPrecondition(format!("配置 TOML 无效: {e:#}")))?;
        self.start_loader(cfg, toml.to_string(), display_name, source_path)
    }

    /// 用结构化配置启动。
    pub fn start_structured(
        &self,
        net: &NetworkConfig,
        display_name: &str,
        source_path: &str,
    ) -> CoreResult<Uuid> {
        let cfg = structured_to_loader(net)?;
        let toml = loader_to_toml(&cfg)?;
        self.start_loader(cfg, toml, display_name, source_path)
    }

    fn start_loader(
        &self,
        cfg: TomlConfigLoader,
        toml: String,
        display_name: &str,
        source_path: &str,
    ) -> CoreResult<Uuid> {
        let id = cfg.get_id();
        if self.exists(id) {
            self.cache.upsert(CachedInstance {
                instance_id: id.to_string(),
                toml,
                display_name: display_name.to_string(),
                source_path: source_path.to_string(),
            })?;
            return Ok(id);
        }
        self.manager
            .run_network_instance(cfg, true, ConfigFileControl::STATIC_CONFIG)
            .map_err(|e| CoreError::Internal(format!("启动实例失败: {e}")))?;

        self.cache.upsert(CachedInstance {
            instance_id: id.to_string(),
            toml,
            display_name: display_name.to_string(),
            source_path: source_path.to_string(),
        })?;
        Ok(id)
    }

    /// 停止（保留配置缓存，便于 Restart）。
    pub fn stop(&self, id: Uuid) -> CoreResult<()> {
        if !self.exists(id) {
            return Ok(());
        }
        self.manager
            .delete_network_instance(vec![id])
            .map_err(|e| CoreError::Internal(format!("停止实例失败: {e}")))?;
        Ok(())
    }

    /// 停止并清除缓存。
    pub fn delete(&self, id: Uuid) -> CoreResult<()> {
        self.stop(id)?;
        self.cache.remove(&id.to_string())?;
        Ok(())
    }

    /// 用缓存配置重启。
    pub fn restart(&self, id: Uuid) -> CoreResult<Uuid> {
        let cached = self
            .cache
            .get_uuid(id)?
            .ok_or_else(|| CoreError::FailedPrecondition(format!("无缓存配置，无法重启: {id}")))?;
        self.stop(id)?;
        self.start_toml(&cached.toml, &cached.display_name, &cached.source_path)
    }

    /// 更新缓存中的 TOML（热补丁后同步）。
    pub fn update_cached_toml(&self, id: Uuid, toml: String) -> CoreResult<()> {
        if let Some(mut rec) = self.cache.get_uuid(id)? {
            rec.toml = toml;
            self.cache.upsert(rec)?;
        }
        Ok(())
    }

    /// 运行信息。
    pub async fn running_info(&self, id: Uuid) -> Option<NetworkInstanceRunningInfo> {
        self.manager.get_network_info(&id).await
    }

    /// Peer 列表（实例在表中时至少含本机）。
    pub async fn list_peers(&self, id: Uuid) -> Vec<PeerSummary> {
        let mut peers = match self.running_info(id).await {
            Some(info) => peer_summaries_from_info(&info),
            None => Vec::new(),
        };
        if !has_local_peer(&peers) {
            let (ipv4, _, hostname) = self.local_addrs(id).await;
            let cached = self.cache.get_uuid(id).ok().flatten();
            let hostname = if hostname.is_empty() {
                cached
                    .as_ref()
                    .map(|c| c.display_name.clone())
                    .unwrap_or_default()
            } else {
                hostname
            };
            let ipv4 = if ipv4.is_empty() {
                cached
                    .as_ref()
                    .map(|c| ipv4_from_toml(&c.toml))
                    .unwrap_or_default()
            } else {
                ipv4
            };
            merge_local_peer(&mut peers, &hostname, &ipv4);
        }
        peers
    }

    /// 本机地址。
    pub async fn local_addrs(&self, id: Uuid) -> (String, String, String) {
        match self.running_info(id).await {
            Some(info) => (
                my_ipv4_from_info(&info),
                String::new(),
                hostname_from_info(&info),
            ),
            None => (String::new(), String::new(), String::new()),
        }
    }

    /// 列出 ID。
    pub fn list_ids(&self) -> Vec<Uuid> {
        self.manager.list_network_instance_ids()
    }

    /// 摘要列表。
    pub async fn list_summaries(&self) -> Vec<InstanceSummary> {
        let mut out = Vec::new();
        for id in self.list_ids() {
            out.push(self.summary_of(id).await);
        }
        out
    }

    /// 单个摘要。
    pub async fn summary_of(&self, id: Uuid) -> InstanceSummary {
        let id_str = id.to_string();
        let display_name = self
            .cache
            .get_uuid(id)
            .ok()
            .and_then(|c| c.map(|c| c.display_name))
            .unwrap_or_default();

        if let Some(info) = self.manager.get_network_info(&id).await {
            let err = error_string(info.error_msg.clone());
            let running = info.running && err.is_empty();
            let state = if running {
                InstanceState::Running
            } else if !err.is_empty() {
                InstanceState::Error
            } else if info.running {
                InstanceState::Starting
            } else {
                InstanceState::Stopped
            };
            let hostname = hostname_from_info(&info);
            return InstanceSummary {
                instance_id: id_str,
                display_name,
                state,
                running,
                error_message: err,
                dev_name: optional_string(info.dev_name.clone()),
                network_name: String::new(),
                hostname,
            };
        }
        if self.list_ids().contains(&id) {
            let err = self
                .manager
                .iter()
                .find(|item| *item.key() == id)
                .and_then(|item| item.get_latest_error_msg())
                .unwrap_or_default();
            return InstanceSummary {
                instance_id: id_str,
                display_name,
                state: InstanceState::Starting,
                running: false,
                error_message: err,
                dev_name: String::new(),
                network_name: String::new(),
                hostname: String::new(),
            };
        }
        InstanceSummary {
            instance_id: id_str,
            display_name,
            state: InstanceState::Stopped,
            running: false,
            error_message: "instance not found".into(),
            dev_name: String::new(),
            network_name: String::new(),
            hostname: String::new(),
        }
    }

    /// 是否在管理表。
    pub fn exists(&self, id: Uuid) -> bool {
        self.list_ids().contains(&id)
    }
}

fn optional_string(v: impl IntoOptionalString) -> String {
    v.into_optional_string()
}

fn error_string(v: impl IntoOptionalString) -> String {
    v.into_optional_string()
}

trait IntoOptionalString {
    fn into_optional_string(self) -> String;
}

impl IntoOptionalString for String {
    fn into_optional_string(self) -> String {
        self
    }
}

impl IntoOptionalString for Option<String> {
    fn into_optional_string(self) -> String {
        self.unwrap_or_default()
    }
}

fn ipv4_from_toml(toml: &str) -> String {
    for line in toml.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("ipv4") else {
            continue;
        };
        let Some(start) = rest.find('"') else {
            continue;
        };
        let inner = &rest[start + 1..];
        let Some(end) = inner.find('"') else {
            continue;
        };
        return inner[..end]
            .split('/')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
    }
    String::new()
}
