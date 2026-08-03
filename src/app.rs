//! 进程级共享状态。

use std::fs;
use std::sync::Arc;

use tracing::{error, info};
use uuid::Uuid;

use crate::auth::TokenStore;
use crate::config::{DataPaths, RuntimeConfig};
use crate::engine::{EngineHandle, EventHub};
use crate::error::CoreResult;
use crate::store::{AutostartStore, InstanceCache, ProfileStore};

/// 全局应用状态（注入各 gRPC Service）。
#[derive(Clone)]
pub struct AppState {
    /// 运行配置。
    pub runtime: Arc<RuntimeConfig>,
    /// 数据路径。
    pub paths: Arc<DataPaths>,
    /// Token 仓库。
    pub tokens: Arc<TokenStore>,
    /// EasyTier 引擎。
    pub engine: EngineHandle,
    /// 本机节点 ID。
    pub node_id: String,
    /// 配置库（含自启标志）。
    pub profiles: Arc<ProfileStore>,
    /// 遗留自启仓库（仅用于启动迁移）。
    pub autostart: Arc<AutostartStore>,
}

impl AppState {
    /// 根据运行配置加载/初始化状态（含引导 token）；随后恢复自启实例。
    pub fn bootstrap(runtime: RuntimeConfig) -> CoreResult<(Self, Option<String>)> {
        let paths = match &runtime.data_dir {
            Some(dir) => DataPaths::from_root(dir.clone())?,
            None => DataPaths::discover()?,
        };
        let tokens = TokenStore::load(paths.tokens_file())?;
        let bootstrap_plain = tokens.ensure_bootstrap(&paths.bootstrap_token_file())?;
        let node_id = load_or_create_node_id(&paths.node_id_file())?;

        let cache = Arc::new(InstanceCache::new());
        let profiles = Arc::new(ProfileStore::open(paths.profiles_dir())?);
        let autostart = Arc::new(AutostartStore::open(paths.autostart_dir())?);

        match profiles.migrate_from_autostart(&autostart) {
            Ok(0) => {}
            Ok(n) => info!(migrated = n, "已将遗留 autostart 迁入 profiles"),
            Err(e) => error!(error = %e, "遗留 autostart 迁移失败"),
        }

        let events = EventHub::new(node_id.clone(), 256);
        let engine = EngineHandle::new(cache).with_events(events);

        let state = Self {
            runtime: Arc::new(runtime),
            paths: Arc::new(paths),
            tokens: Arc::new(tokens),
            engine,
            node_id,
            profiles,
            autostart,
        };

        state.restore_autostart_instances();

        Ok((state, bootstrap_plain))
    }

    /// 遍历配置库中 autostart=true 的条目并拉起。
    fn restore_autostart_instances(&self) {
        let configs = match self.profiles.load_autostart_configs() {
            Ok(list) => list,
            Err(e) => {
                error!(error = %e, "加载自启配置失败，跳过自启恢复");
                return;
            }
        };
        for rec in configs {
            match self
                .engine
                .start_toml(&rec.toml, &rec.display_name, "")
            {
                Ok(id) => info!(instance_id = %id, "自启实例已恢复"),
                Err(e) => error!(
                    instance_id = %rec.instance_id,
                    error = %e,
                    "自启实例恢复失败"
                ),
            }
        }
    }
}

fn load_or_create_node_id(path: &std::path::Path) -> CoreResult<String> {
    if path.exists() {
        let id = fs::read_to_string(path)?.trim().to_string();
        if !id.is_empty() {
            return Ok(id);
        }
    }
    let id = Uuid::new_v4().to_string();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{id}\n"))?;
    Ok(id)
}
