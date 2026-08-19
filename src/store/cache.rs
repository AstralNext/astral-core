//! 运行中 / 已停实例的配置缓存（内存 + 可选落盘）。

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{CoreError, CoreResult};

/// 单条缓存记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedInstance {
    /// 实例 UUID。
    pub instance_id: String,
    /// 完整 TOML（Restart / 自启恢复用）。
    pub toml: String,
    /// UI 显示名。
    pub display_name: String,
    /// 来源路径（可选）。
    pub source_path: String,
    /// 本次运行开始时刻（Unix 毫秒）；停止后清空。
    #[serde(default)]
    pub started_at_unix_ms: Option<u64>,
}

/// 线程安全的实例配置缓存。
#[derive(Debug, Default)]
pub struct InstanceCache {
    inner: Mutex<HashMap<String, CachedInstance>>,
}

impl InstanceCache {
    /// 空缓存。
    pub fn new() -> Self {
        Self::default()
    }

    /// 写入或覆盖。
    pub fn upsert(&self, rec: CachedInstance) -> CoreResult<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| CoreError::Internal("instance cache 锁毒化".into()))?;
        g.insert(rec.instance_id.clone(), rec);
        Ok(())
    }

    /// 按 ID 读取。
    pub fn get(&self, instance_id: &str) -> CoreResult<Option<CachedInstance>> {
        let g = self
            .inner
            .lock()
            .map_err(|_| CoreError::Internal("instance cache 锁毒化".into()))?;
        Ok(g.get(instance_id).cloned())
    }

    /// 按 UUID 读取。
    pub fn get_uuid(&self, id: Uuid) -> CoreResult<Option<CachedInstance>> {
        self.get(&id.to_string())
    }

    /// 删除缓存（真正 Delete 且不再保留时）。
    pub fn remove(&self, instance_id: &str) -> CoreResult<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| CoreError::Internal("instance cache 锁毒化".into()))?;
        g.remove(instance_id);
        Ok(())
    }

    /// 列出全部。
    pub fn list(&self) -> CoreResult<Vec<CachedInstance>> {
        let g = self
            .inner
            .lock()
            .map_err(|_| CoreError::Internal("instance cache 锁毒化".into()))?;
        Ok(g.values().cloned().collect())
    }
}
