//! 控制端鉴权：共享 join token + 已颁发设备凭证。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{CoreError, CoreResult};
use crate::pb::AgentHandshakeRequest;

#[derive(Debug, Default, Serialize, Deserialize)]
struct DeviceFile {
    /// node_id -> credential hash hex
    devices: HashMap<String, String>,
}

/// 控制端认证状态。
#[derive(Debug)]
pub struct ControllerAuth {
    join_token: String,
    path: PathBuf,
    inner: Mutex<DeviceFile>,
}

impl ControllerAuth {
    /// 加载或创建设备库。
    pub fn open(data_dir: &Path, join_token: String) -> CoreResult<Self> {
        if join_token.is_empty() {
            return Err(CoreError::InvalidArgument("controller token 不能为空".into()));
        }
        fs::create_dir_all(data_dir)?;
        let path = data_dir.join("controller_devices.json");
        let file = if path.exists() {
            serde_json::from_str(&fs::read_to_string(&path)?)?
        } else {
            DeviceFile::default()
        };
        Ok(Self {
            join_token,
            path,
            inner: Mutex::new(file),
        })
    }

    /// 控制端 attestation（节点用来校验对方也持有同一密钥）。
    pub fn attestation(&self) -> &str {
        &self.join_token
    }

    /// 校验握手；成功则返回 (node_id, 可选新 device_credential 明文)。
    pub fn authenticate(
        &self,
        req: &AgentHandshakeRequest,
    ) -> CoreResult<(String, Option<String>)> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| CoreError::Internal("controller auth 锁毒化".into()))?;

        if !req.device_credential.is_empty() {
            let node_id = if req.node_id.is_empty() {
                return Err(CoreError::InvalidArgument(
                    "重连时必须携带 node_id".into(),
                ));
            } else {
                req.node_id.clone()
            };
            let Some(hash) = guard.devices.get(&node_id) else {
                return Err(CoreError::FailedPrecondition(
                    "未知设备凭证 / 节点未注册".into(),
                ));
            };
            if !hash_eq(hash, &req.device_credential) {
                return Err(CoreError::FailedPrecondition("设备凭证无效".into()));
            }
            return Ok((node_id, None));
        }

        if req.enroll_token.is_empty() || req.enroll_token != self.join_token {
            return Err(CoreError::FailedPrecondition(
                "enroll_token 无效（与控制端 --token 不一致）".into(),
            ));
        }

        let node_id = if req.node_id.is_empty() {
            format!("node-{}", uuid::Uuid::new_v4())
        } else {
            req.node_id.clone()
        };
        // 禁止用 enroll 覆盖已注册节点（防持有 join token 劫持身份）
        if guard.devices.contains_key(&node_id) {
            return Err(CoreError::FailedPrecondition(format!(
                "节点已注册，请使用 device_credential 重连: {node_id}"
            )));
        }
        let plain = issue_credential();
        guard
            .devices
            .insert(node_id.clone(), hash_hex(&plain));
        self.persist(&guard)?;
        Ok((node_id, Some(plain)))
    }

    fn persist(&self, file: &DeviceFile) -> CoreResult<()> {
        let raw = serde_json::to_string_pretty(file)?;
        fs::write(&self.path, raw)?;
        Ok(())
    }
}

fn issue_credential() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    format!("adev_{}", hex::encode(buf))
}

fn hash_hex(plain: &str) -> String {
    let mut h = Sha256::new();
    h.update(plain.as_bytes());
    hex::encode(h.finalize())
}

fn hash_eq(stored_hex: &str, plain: &str) -> bool {
    stored_hex.eq_ignore_ascii_case(&hash_hex(plain))
}
