//! 迁移 / 安装阶段状态（异常中断后可恢复）。
#![allow(missing_docs)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::health::default_data_root;

/// 安装 / 更新阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrationPhase {
    Preflight,
    StopOld,
    MigrateData,
    StageNewVersion,
    SwitchCurrent,
    InstallOrUpdateService,
    VerifyRunning,
    CommitRegistry,
    Done,
}

/// 持久化到数据根的迁移状态。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationState {
    pub phase: MigrationPhase,
    pub target_version: Option<String>,
    pub started_at: String,
    pub updated_at: String,
    pub message: Option<String>,
}

fn state_path(root: &Path) -> PathBuf {
    root.join("service_migration_state.json")
}

pub fn load_state() -> Result<Option<MigrationState>> {
    let root = default_data_root()?;
    let path = state_path(&root);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)
        .with_context(|| format!("读取迁移状态失败: {}", path.display()))?;
    Ok(Some(serde_json::from_str(&text)?))
}

pub fn save_state(state: &MigrationState) -> Result<()> {
    let root = default_data_root()?;
    fs::create_dir_all(&root)?;
    let path = state_path(&root);
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(state)?)?;
    fs::rename(&tmp, &path).with_context(|| format!("写入迁移状态失败: {}", path.display()))
}

pub fn clear_state() -> Result<()> {
    let root = default_data_root()?;
    let path = state_path(&root);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn begin_phase(
    phase: MigrationPhase,
    target_version: Option<String>,
    message: Option<String>,
) -> Result<()> {
    let now = chrono_lite_now();
    let existing = load_state().ok().flatten();
    let started_at = existing
        .as_ref()
        .map(|s| s.started_at.clone())
        .unwrap_or_else(|| now.clone());
    save_state(&MigrationState {
        phase,
        target_version,
        started_at,
        updated_at: now,
        message,
    })
}

fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn resume_if_incomplete<F>(mut step: F) -> Result<()>
where
    F: FnMut(MigrationPhase) -> Result<()>,
{
    if let Some(state) = load_state()? {
        if state.phase != MigrationPhase::Done {
            step(state.phase)?;
        }
    }
    Ok(())
}
