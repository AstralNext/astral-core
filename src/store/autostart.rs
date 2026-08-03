//! 自启登记：落盘 TOML + 元数据索引。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CoreResult;
use crate::pb::AutostartEntry;
use crate::store::CachedInstance;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutostartIndex {
    entries: Vec<AutostartMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutostartMeta {
    instance_id: String,
    display_name: String,
    source_path: String,
    /// 相对 autostart 目录的文件名。
    config_file: String,
}

/// 自启存储。
#[derive(Debug, Clone)]
pub struct AutostartStore {
    dir: PathBuf,
    index_path: PathBuf,
}

impl AutostartStore {
    /// 打开目录（不存在则创建）。
    pub fn open(dir: PathBuf) -> CoreResult<Self> {
        fs::create_dir_all(&dir)?;
        let index_path = dir.join("index.json");
        Ok(Self { dir, index_path })
    }

    /// 登记自启（写入配置文件 + 索引）。
    pub fn set(&self, rec: &CachedInstance) -> CoreResult<()> {
        let file = format!("{}.toml", rec.instance_id);
        fs::write(self.dir.join(&file), &rec.toml)?;
        let mut idx = self.load_index()?;
        idx.entries.retain(|e| e.instance_id != rec.instance_id);
        idx.entries.push(AutostartMeta {
            instance_id: rec.instance_id.clone(),
            display_name: rec.display_name.clone(),
            source_path: rec.source_path.clone(),
            config_file: file,
        });
        self.save_index(&idx)?;
        Ok(())
    }

    /// 取消自启。
    pub fn clear(&self, instance_id: &str) -> CoreResult<()> {
        let mut idx = self.load_index()?;
        if let Some(pos) = idx.entries.iter().position(|e| e.instance_id == instance_id) {
            let file = idx.entries[pos].config_file.clone();
            idx.entries.remove(pos);
            let _ = fs::remove_file(self.dir.join(file));
            self.save_index(&idx)?;
        }
        Ok(())
    }

    /// 是否已登记。
    pub fn is_enabled(&self, instance_id: &str) -> CoreResult<bool> {
        Ok(self
            .load_index()?
            .entries
            .iter()
            .any(|e| e.instance_id == instance_id))
    }

    /// 列出 API 用条目。
    pub fn list_entries(&self) -> CoreResult<Vec<AutostartEntry>> {
        Ok(self
            .load_index()?
            .entries
            .into_iter()
            .map(|e| AutostartEntry {
                instance_id: e.instance_id,
                source_path: e.source_path,
                display_name: e.display_name,
                has_config: true,
            })
            .collect())
    }

    /// 加载全部待恢复配置。
    pub fn load_all_configs(&self) -> CoreResult<Vec<CachedInstance>> {
        let idx = self.load_index()?;
        let mut out = Vec::new();
        for e in idx.entries {
            let path = self.dir.join(&e.config_file);
            if !path.exists() {
                continue;
            }
            let toml = fs::read_to_string(&path)?;
            out.push(CachedInstance {
                instance_id: e.instance_id,
                toml,
                display_name: e.display_name,
                source_path: e.source_path,
            });
        }
        Ok(out)
    }

    fn load_index(&self) -> CoreResult<AutostartIndex> {
        if !self.index_path.exists() {
            return Ok(AutostartIndex { entries: vec![] });
        }
        let raw = fs::read_to_string(&self.index_path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    fn save_index(&self, idx: &AutostartIndex) -> CoreResult<()> {
        if let Some(p) = self.index_path.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(&self.index_path, serde_json::to_string_pretty(idx)?)?;
        Ok(())
    }

    /// 根目录（调试）。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 清空全部自启登记（迁移到 ProfileStore 后调用）。
    pub fn clear_all(&self) -> CoreResult<()> {
        let idx = self.load_index()?;
        for e in &idx.entries {
            let _ = fs::remove_file(self.dir.join(&e.config_file));
        }
        self.save_index(&AutostartIndex { entries: vec![] })?;
        Ok(())
    }
}
