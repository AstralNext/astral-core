//! 进程级共享状态。

use std::fs;
use std::sync::Arc;

use uuid::Uuid;

use crate::config::{DataPaths, RuntimeConfig};
use crate::engine::EngineHandle;
use crate::error::CoreResult;
use crate::logging::LogHub;
use crate::store::InstanceCache;

/// 全局应用状态（注入各 gRPC Service）。
#[derive(Clone)]
pub struct AppState {
    /// 运行配置。
    pub runtime: Arc<RuntimeConfig>,
    /// 数据路径。
    pub paths: Arc<DataPaths>,
    /// EasyTier 引擎。
    pub engine: EngineHandle,
    /// 本机节点 ID。
    pub node_id: String,
    /// 日志中枢。
    pub logs: LogHub,
}

impl AppState {
    /// 根据运行配置加载/初始化状态。
    pub fn bootstrap(runtime: RuntimeConfig) -> CoreResult<Self> {
        Self::bootstrap_with_log_filter(runtime, crate::logging::DEFAULT_LOG_FILTER)
    }

    /// 与 [`Self::bootstrap`] 相同，并按 `log_filter` 安装 tracing。
    pub fn bootstrap_with_log_filter(
        runtime: RuntimeConfig,
        log_filter: &str,
    ) -> CoreResult<Self> {
        let logs = LogHub::install(log_filter);
        let paths = match &runtime.data_dir {
            Some(dir) => DataPaths::from_root(dir.clone()),
            None => DataPaths::discover(),
        }?;
        let node_id = load_or_create_node_id(&paths.node_id_file())?;

        let cache = Arc::new(InstanceCache::new());
        let engine = EngineHandle::new(cache);

        Ok(Self {
            runtime: Arc::new(runtime),
            paths: Arc::new(paths),
            engine,
            node_id,
            logs,
        })
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
