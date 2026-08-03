//! Service 层小工具。

use crate::error::CoreError;
use crate::pb::{InstanceConfigSource, InstanceRef, NetworkConfig, NodeRef};

/// 解析后的实例配置来源。
pub enum ResolvedConfig {
    /// TOML 原文。
    Toml(String),
    /// 结构化 NetworkConfig。
    Structured(NetworkConfig),
}

/// 解析实例 UUID。
pub fn parse_instance_id(id: &str) -> Result<uuid::Uuid, CoreError> {
    uuid::Uuid::parse_str(id)
        .map_err(|_| CoreError::InvalidArgument(format!("无效 instance_id: {id}")))
}

/// 从 InstanceRef 取 instance_id。
pub fn require_instance_id(r: &Option<InstanceRef>) -> Result<String, CoreError> {
    let r = r
        .as_ref()
        .ok_or_else(|| CoreError::InvalidArgument("缺少 instance".into()))?;
    if r.instance_id.is_empty() {
        return Err(CoreError::InvalidArgument("instance_id 为空".into()));
    }
    Ok(r.instance_id.clone())
}

/// 校验 NodeRef：空或本机 node_id 均接受。
pub fn ensure_local_node(node: &Option<NodeRef>, local_id: &str) -> Result<(), CoreError> {
    let Some(n) = node else {
        return Ok(());
    };
    if n.node_id.is_empty() || n.node_id == local_id {
        Ok(())
    } else {
        Err(CoreError::InvalidArgument(format!(
            "单节点 Core 不支持 node_id={}",
            n.node_id
        )))
    }
}

/// 解析配置 oneof（toml 或 structured）。
pub fn resolve_config(cfg: &Option<InstanceConfigSource>) -> Result<ResolvedConfig, CoreError> {
    let cfg = cfg
        .as_ref()
        .ok_or_else(|| CoreError::InvalidArgument("缺少 config".into()))?;
    match cfg.source.as_ref() {
        Some(crate::pb::instance_config_source::Source::Toml(t)) if !t.is_empty() => {
            Ok(ResolvedConfig::Toml(t.clone()))
        }
        Some(crate::pb::instance_config_source::Source::Structured(s)) => {
            Ok(ResolvedConfig::Structured(s.clone()))
        }
        _ => Err(CoreError::InvalidArgument(
            "config 为空（需 toml 或 structured）".into(),
        )),
    }
}

/// Unix 秒 → pb Timestamp。
pub fn ts_from_unix(secs: i64) -> crate::pb::Timestamp {
    crate::pb::Timestamp {
        seconds: secs,
        nanos: 0,
    }
}

/// 将 astral PatchAction 映射为 ET ConfigPatchAction（i32）。
pub fn et_patch_action(action: i32) -> i32 {
    // astral: UNSPECIFIED=0 SET=1 ADD=2 REMOVE=3 CLEAR=4
    // et: ADD=0 REMOVE=1 CLEAR=2
    match action {
        2 => 0, // ADD
        3 => 1, // REMOVE
        4 => 2, // CLEAR
        _ => 0,
    }
}
