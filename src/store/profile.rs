//! 配置库：全部实例配置的权威落盘（含自启标志）。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, CoreResult};
use crate::pb::{AutostartEntry, ProfileSummary};
use crate::store::autostart::AutostartStore;
use crate::store::CachedInstance;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfileIndex {
    entries: Vec<ProfileMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfileMeta {
    instance_id: String,
    display_name: String,
    #[serde(default)]
    group: String,
    #[serde(default)]
    autostart: bool,
    #[serde(default)]
    updated_at_unix: i64,
    /// 相对 profiles 目录的文件名。
    config_file: String,
}

/// 已存配置记录（含 toml）。
#[derive(Debug, Clone)]
pub struct ProfileRecord {
    /// 实例 UUID。
    pub instance_id: String,
    /// 显示名。
    pub display_name: String,
    /// 可选分组。
    pub group: String,
    /// 是否开机自启。
    pub autostart: bool,
    /// 更新时间（Unix 秒）。
    pub updated_at_unix: i64,
    /// TOML 原文。
    pub toml: String,
}

impl ProfileRecord {
    /// 转为 API 摘要。
    pub fn to_summary(&self) -> ProfileSummary {
        ProfileSummary {
            instance_id: self.instance_id.clone(),
            display_name: self.display_name.clone(),
            group: self.group.clone(),
            autostart: self.autostart,
            updated_at_unix: self.updated_at_unix,
        }
    }

    /// 转为内存缓存条目。
    pub fn to_cached(&self) -> CachedInstance {
        CachedInstance {
            instance_id: self.instance_id.clone(),
            toml: self.toml.clone(),
            display_name: self.display_name.clone(),
            source_path: String::new(),
        }
    }
}

/// 配置库存储。
#[derive(Debug, Clone)]
pub struct ProfileStore {
    dir: PathBuf,
    index_path: PathBuf,
}

impl ProfileStore {
    /// 打开目录（不存在则创建）。
    pub fn open(dir: PathBuf) -> CoreResult<Self> {
        fs::create_dir_all(&dir)?;
        let index_path = dir.join("index.json");
        Ok(Self { dir, index_path })
    }

    /// 根目录。
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// 从旧 AutostartStore 迁入（幂等；已存在同 id 则仅打开 autostart）。
    pub fn migrate_from_autostart(&self, legacy: &AutostartStore) -> CoreResult<usize> {
        let configs = legacy.load_all_configs()?;
        let mut n = 0;
        for rec in configs {
            let existing = self.get(&rec.instance_id)?;
            if let Some(mut cur) = existing {
                if !cur.autostart {
                    cur.autostart = true;
                    if cur.display_name.is_empty() {
                        cur.display_name = rec.display_name;
                    }
                    if cur.toml.is_empty() {
                        cur.toml = rec.toml;
                    }
                    self.write_record(&cur, true)?;
                    n += 1;
                }
                continue;
            }
            self.write_record(
                &ProfileRecord {
                    instance_id: rec.instance_id,
                    display_name: rec.display_name,
                    group: String::new(),
                    autostart: true,
                    updated_at_unix: now_unix(),
                    toml: rec.toml,
                },
                false,
            )?;
            n += 1;
        }
        if n > 0 {
            let _ = legacy.clear_all();
        }
        Ok(n)
    }

    /// 列出摘要；`autostart_only` 时仅返回自启条目。
    pub fn list(&self, autostart_only: bool) -> CoreResult<Vec<ProfileSummary>> {
        let idx = self.load_index()?;
        Ok(idx
            .entries
            .into_iter()
            .filter(|e| !autostart_only || e.autostart)
            .map(|e| ProfileSummary {
                instance_id: e.instance_id,
                display_name: e.display_name,
                group: e.group,
                autostart: e.autostart,
                updated_at_unix: e.updated_at_unix,
            })
            .collect())
    }

    /// 列为兼容用的 AutostartEntry。
    pub fn list_autostart_entries(&self) -> CoreResult<Vec<AutostartEntry>> {
        Ok(self
            .list(true)?
            .into_iter()
            .map(|p| AutostartEntry {
                instance_id: p.instance_id,
                source_path: String::new(),
                display_name: p.display_name,
                has_config: true,
            })
            .collect())
    }

    /// 按 id 读取完整记录。
    pub fn get(&self, instance_id: &str) -> CoreResult<Option<ProfileRecord>> {
        let idx = self.load_index()?;
        let Some(meta) = idx.entries.iter().find(|e| e.instance_id == instance_id) else {
            return Ok(None);
        };
        let path = self.dir.join(&meta.config_file);
        if !path.exists() {
            return Ok(None);
        }
        let toml = fs::read_to_string(&path)?;
        Ok(Some(ProfileRecord {
            instance_id: meta.instance_id.clone(),
            display_name: meta.display_name.clone(),
            group: meta.group.clone(),
            autostart: meta.autostart,
            updated_at_unix: meta.updated_at_unix,
            toml,
        }))
    }

    /// 必须存在，否则 NotFound。
    pub fn require(&self, instance_id: &str) -> CoreResult<ProfileRecord> {
        self.get(instance_id)?
            .ok_or_else(|| CoreError::NotFound(format!("配置不存在: {instance_id}")))
    }

    /// 写入/覆盖。`preserve_autostart_if_exists`：已存在时保留原 autostart。
    pub fn upsert(
        &self,
        instance_id: &str,
        toml: &str,
        display_name: &str,
        group: &str,
        autostart: bool,
        preserve_autostart_if_exists: bool,
    ) -> CoreResult<ProfileRecord> {
        let existing = self.get(instance_id)?;
        let keep_auto = if preserve_autostart_if_exists {
            existing
                .as_ref()
                .map(|e| e.autostart)
                .unwrap_or(autostart)
        } else {
            autostart
        };
        let name = if !display_name.is_empty() {
            display_name.to_string()
        } else {
            existing
                .as_ref()
                .map(|e| e.display_name.clone())
                .unwrap_or_default()
        };
        let grp = if !group.is_empty() {
            group.to_string()
        } else {
            existing
                .as_ref()
                .map(|e| e.group.clone())
                .unwrap_or_default()
        };
        let rec = ProfileRecord {
            instance_id: instance_id.to_string(),
            display_name: name,
            group: grp,
            autostart: keep_auto,
            updated_at_unix: now_unix(),
            toml: toml.to_string(),
        };
        self.write_record(&rec, existing.is_some())?;
        Ok(rec)
    }

    /// 设置自启标志。
    pub fn set_autostart(&self, instance_id: &str, enabled: bool) -> CoreResult<ProfileRecord> {
        let mut rec = self.require(instance_id)?;
        rec.autostart = enabled;
        rec.updated_at_unix = now_unix();
        self.write_record(&rec, true)?;
        Ok(rec)
    }

    /// 是否已登记自启。
    pub fn is_autostart(&self, instance_id: &str) -> CoreResult<bool> {
        Ok(self
            .get(instance_id)?
            .map(|r| r.autostart)
            .unwrap_or(false))
    }

    /// 删除配置；不存在返回 false。
    pub fn delete(&self, instance_id: &str) -> CoreResult<bool> {
        let mut idx = self.load_index()?;
        let Some(pos) = idx.entries.iter().position(|e| e.instance_id == instance_id) else {
            return Ok(false);
        };
        let file = idx.entries[pos].config_file.clone();
        idx.entries.remove(pos);
        let _ = fs::remove_file(self.dir.join(file));
        self.save_index(&idx)?;
        Ok(true)
    }

    /// 加载全部待自启恢复的配置。
    pub fn load_autostart_configs(&self) -> CoreResult<Vec<ProfileRecord>> {
        let idx = self.load_index()?;
        let mut out = Vec::new();
        for e in idx.entries.into_iter().filter(|e| e.autostart) {
            let path = self.dir.join(&e.config_file);
            if !path.exists() {
                continue;
            }
            let toml = fs::read_to_string(&path)?;
            out.push(ProfileRecord {
                instance_id: e.instance_id,
                display_name: e.display_name,
                group: e.group,
                autostart: true,
                updated_at_unix: e.updated_at_unix,
                toml,
            });
        }
        Ok(out)
    }

    fn write_record(&self, rec: &ProfileRecord, _existed: bool) -> CoreResult<()> {
        let file = format!("{}.toml", rec.instance_id);
        fs::write(self.dir.join(&file), &rec.toml)?;
        let mut idx = self.load_index()?;
        idx.entries.retain(|e| e.instance_id != rec.instance_id);
        idx.entries.push(ProfileMeta {
            instance_id: rec.instance_id.clone(),
            display_name: rec.display_name.clone(),
            group: rec.group.clone(),
            autostart: rec.autostart,
            updated_at_unix: rec.updated_at_unix,
            config_file: file,
        });
        self.save_index(&idx)?;
        Ok(())
    }

    fn load_index(&self) -> CoreResult<ProfileIndex> {
        if !self.index_path.exists() {
            return Ok(ProfileIndex { entries: vec![] });
        }
        let raw = fs::read_to_string(&self.index_path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    fn save_index(&self, idx: &ProfileIndex) -> CoreResult<()> {
        if let Some(p) = self.index_path.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(&self.index_path, serde_json::to_string_pretty(idx)?)?;
        Ok(())
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
