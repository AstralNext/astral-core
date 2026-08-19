//! 运行中 / 已停实例的配置缓存（内存 + 可选落盘）。
//!
//! 落盘只保留「上次在跑」的记录：开机后按此自动拉起。用户主动停止后从文件删除。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tracing::warn;
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

impl CachedInstance {
    /// 是否应在内核启动时自动拉起。
    pub fn desired_running(&self) -> bool {
        self.started_at_unix_ms.is_some() && !self.toml.trim().is_empty()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistFile {
    version: u32,
    instances: Vec<CachedInstance>,
}

/// 线程安全的实例配置缓存。
#[derive(Debug)]
pub struct InstanceCache {
    inner: Mutex<HashMap<String, CachedInstance>>,
    persist_path: Option<PathBuf>,
}

impl Default for InstanceCache {
    fn default() -> Self {
        Self::new()
    }
}

impl InstanceCache {
    /// 仅内存，不落盘（测试 / 临时进程）。
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            persist_path: None,
        }
    }

    /// 从磁盘加载；文件不存在则为空。损坏文件会改名为 `.corrupt` 后视为空。
    pub fn load_or_create(path: PathBuf) -> CoreResult<Self> {
        let cache = Self {
            inner: Mutex::new(HashMap::new()),
            persist_path: Some(path.clone()),
        };
        if !path.exists() {
            return Ok(cache);
        }
        match load_persist_file(&path) {
            Ok(file) => {
                let mut g = cache.lock()?;
                for rec in file.instances {
                    g.insert(rec.instance_id.clone(), rec);
                }
            }
            Err(e) => {
                let corrupt = path.with_extension("json.corrupt");
                warn!(
                    path = %path.display(),
                    corrupt = %corrupt.display(),
                    error = %e,
                    "实例缓存损坏，已忽略"
                );
                let _ = std::fs::rename(&path, &corrupt);
            }
        }
        Ok(cache)
    }

    fn lock(&self) -> CoreResult<std::sync::MutexGuard<'_, HashMap<String, CachedInstance>>> {
        self.inner
            .lock()
            .map_err(|_| CoreError::Internal("instance cache 锁毒化".into()))
    }

    /// 写入或覆盖。
    pub fn upsert(&self, rec: CachedInstance) -> CoreResult<()> {
        {
            let mut g = self.lock()?;
            g.insert(rec.instance_id.clone(), rec);
        }
        self.persist_best_effort();
        Ok(())
    }

    /// 按 ID 读取。
    pub fn get(&self, instance_id: &str) -> CoreResult<Option<CachedInstance>> {
        let g = self.lock()?;
        Ok(g.get(instance_id).cloned())
    }

    /// 按 UUID 读取。
    pub fn get_uuid(&self, id: Uuid) -> CoreResult<Option<CachedInstance>> {
        self.get(&id.to_string())
    }

    /// 删除缓存（真正 Delete 且不再保留时）。
    pub fn remove(&self, instance_id: &str) -> CoreResult<()> {
        {
            let mut g = self.lock()?;
            g.remove(instance_id);
        }
        self.persist_best_effort();
        Ok(())
    }

    /// 列出全部（含已停止、仍留在内存中的记录）。
    pub fn list(&self) -> CoreResult<Vec<CachedInstance>> {
        let g = self.lock()?;
        Ok(g.values().cloned().collect())
    }

    /// 应在开机时自动拉起的记录。
    pub fn desired_running(&self) -> CoreResult<Vec<CachedInstance>> {
        Ok(self
            .list()?
            .into_iter()
            .filter(CachedInstance::desired_running)
            .collect())
    }

    fn persist_best_effort(&self) {
        if let Err(e) = self.persist() {
            warn!(error = %e, "写入实例缓存失败");
        }
    }

    fn persist(&self) -> CoreResult<()> {
        let Some(path) = &self.persist_path else {
            return Ok(());
        };
        let instances = {
            let g = self.lock()?;
            g.values()
                .filter(|c| c.desired_running())
                .cloned()
                .collect::<Vec<_>>()
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = PersistFile {
            version: 1,
            instances,
        };
        let bytes = serde_json::to_vec_pretty(&file)?;
        write_atomic(path, &bytes)?;
        Ok(())
    }
}

fn load_persist_file(path: &Path) -> CoreResult<PersistFile> {
    let raw = std::fs::read_to_string(path)?;
    let file: PersistFile = serde_json::from_str(&raw)?;
    if file.version == 0 {
        return Err(CoreError::Internal("实例缓存 version 无效".into()));
    }
    Ok(file)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample(id: &str, running: bool) -> CachedInstance {
        CachedInstance {
            instance_id: id.into(),
            toml: format!("instance_id = \"{id}\"\nhostname = \"h\"\n"),
            display_name: "demo".into(),
            source_path: "/tmp/demo.toml".into(),
            started_at_unix_ms: if running {
                Some(1_700_000_000_000)
            } else {
                None
            },
        }
    }

    #[test]
    fn persists_running_and_drops_stopped() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("instance_cache.json");
        let cache = InstanceCache::load_or_create(path.clone()).unwrap();

        cache
            .upsert(sample("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", true))
            .unwrap();
        cache
            .upsert(sample("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb", false))
            .unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let file: PersistFile = serde_json::from_str(&raw).unwrap();
        assert_eq!(file.instances.len(), 1);
        assert_eq!(
            file.instances[0].instance_id,
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
        );

        cache
            .upsert(CachedInstance {
                started_at_unix_ms: None,
                ..sample("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", true)
            })
            .unwrap();
        let file: PersistFile =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(file.instances.is_empty());
    }

    #[test]
    fn load_roundtrip_desired_running() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("instance_cache.json");
        let cache = InstanceCache::load_or_create(path.clone()).unwrap();
        cache
            .upsert(sample("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", true))
            .unwrap();
        drop(cache);

        let loaded = InstanceCache::load_or_create(path).unwrap();
        let desired = loaded.desired_running().unwrap();
        assert_eq!(desired.len(), 1);
        assert_eq!(desired[0].display_name, "demo");
        assert_eq!(desired[0].source_path, "/tmp/demo.toml");
    }

    #[test]
    fn corrupt_file_starts_empty() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("instance_cache.json");
        std::fs::write(&path, "{not json").unwrap();
        let cache = InstanceCache::load_or_create(path.clone()).unwrap();
        assert!(cache.list().unwrap().is_empty());
        assert!(path.with_extension("json.corrupt").exists());
    }
}
