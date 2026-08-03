//! Token 持久化存储（只存哈希，明文仅创建时返回）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::{CoreError, CoreResult};

/// 磁盘上的 token 记录（无明文）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRecord {
    /// Token 稳定 ID。
    pub token_id: String,
    /// 显示名。
    pub name: String,
    /// 明文前缀（便于辨认），如 ask_ab12。
    pub prefix: String,
    /// SHA-256(hex) 哈希。
    pub hash_hex: String,
    /// 创建时间 Unix 秒。
    pub created_at_unix: i64,
    /// 过期时间 Unix 秒；None 表示不过期。
    pub expires_at_unix: Option<i64>,
    /// 是否已吊销。
    pub revoked: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct TokenFile {
    tokens: Vec<TokenRecord>,
}

/// 线程安全的 Token 仓库。
#[derive(Debug)]
pub struct TokenStore {
    path: PathBuf,
    inner: Mutex<TokenFile>,
}

impl TokenStore {
    /// 从文件加载；文件不存在则空仓库。
    pub fn load(path: impl Into<PathBuf>) -> CoreResult<Self> {
        let path = path.into();
        let file = if path.exists() {
            let raw = fs::read_to_string(&path)?;
            serde_json::from_str(&raw)?
        } else {
            TokenFile::default()
        };
        Ok(Self {
            path,
            inner: Mutex::new(file),
        })
    }

    /// 当前有效（未吊销且未过期）token 数量。
    pub fn active_count(&self) -> CoreResult<usize> {
        let now = now_unix();
        let guard = self.inner.lock().map_err(|_| CoreError::Internal("token 锁毒化".into()))?;
        Ok(guard
            .tokens
            .iter()
            .filter(|t| !t.revoked && !is_expired(t, now))
            .count())
    }

    /// 校验明文 token 是否有效。
    pub fn verify_plaintext(&self, token: &str) -> CoreResult<bool> {
        let hash = hash_token(token);
        let now = now_unix();
        let guard = self.inner.lock().map_err(|_| CoreError::Internal("token 锁毒化".into()))?;
        Ok(guard.tokens.iter().any(|t| {
            !t.revoked && !is_expired(t, now) && t.hash_hex.eq_ignore_ascii_case(&hash)
        }))
    }

    /// 创建新 token；返回 (记录, 明文)。明文仅此一次。
    pub fn create(&self, name: impl Into<String>, expires_at_unix: Option<i64>) -> CoreResult<(TokenRecord, String)> {
        let plaintext = generate_token_plaintext();
        let prefix: String = plaintext.chars().take(12).collect();
        let record = TokenRecord {
            token_id: Uuid::new_v4().to_string(),
            name: name.into(),
            prefix,
            hash_hex: hash_token(&plaintext),
            created_at_unix: now_unix(),
            expires_at_unix,
            revoked: false,
        };
        {
            let mut guard = self.inner.lock().map_err(|_| CoreError::Internal("token 锁毒化".into()))?;
            guard.tokens.push(record.clone());
            self.persist_locked(&guard)?;
        }
        Ok((record, plaintext))
    }

    /// 列出元数据。
    pub fn list(&self, include_revoked: bool) -> CoreResult<Vec<TokenRecord>> {
        let guard = self.inner.lock().map_err(|_| CoreError::Internal("token 锁毒化".into()))?;
        Ok(guard
            .tokens
            .iter()
            .filter(|t| include_revoked || !t.revoked)
            .cloned()
            .collect())
    }

    /// 吊销；禁止吊销最后一把有效 token。
    pub fn revoke(&self, token_id: &str) -> CoreResult<()> {
        let now = now_unix();
        let mut guard = self.inner.lock().map_err(|_| CoreError::Internal("token 锁毒化".into()))?;
        let active_before = guard
            .tokens
            .iter()
            .filter(|t| !t.revoked && !is_expired(t, now))
            .count();
        let Some(rec) = guard.tokens.iter_mut().find(|t| t.token_id == token_id) else {
            return Err(CoreError::NotFound(format!("token_id={token_id}")));
        };
        if rec.revoked {
            return Ok(());
        }
        let was_active = !is_expired(rec, now);
        if was_active && active_before <= 1 {
            return Err(CoreError::FailedPrecondition(
                "禁止吊销最后一把有效 API Token（LAST_TOKEN_REVOKE_FORBIDDEN）".into(),
            ));
        }
        rec.revoked = true;
        self.persist_locked(&guard)?;
        Ok(())
    }

    /// 若仓库为空则创建引导 token，并可选写入明文文件。
    pub fn ensure_bootstrap(&self, bootstrap_file: &Path) -> CoreResult<Option<String>> {
        if self.active_count()? > 0 {
            return Ok(None);
        }
        let (rec, plain) = self.create("bootstrap", None)?;
        fs::write(bootstrap_file, format!("{plain}\n"))?;
        tracing::warn!(
            token_id = %rec.token_id,
            path = %bootstrap_file.display(),
            "已生成引导 API Token（明文已写入文件，请妥善保存；列表接口不会再次返回明文）"
        );
        Ok(Some(plain))
    }

    fn persist_locked(&self, file: &TokenFile) -> CoreResult<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(file)?;
        fs::write(&self.path, raw)?;
        Ok(())
    }
}

fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

fn generate_token_plaintext() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    format!("ask_{}", hex::encode(buf))
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn is_expired(t: &TokenRecord, now: i64) -> bool {
    t.expires_at_unix.is_some_and(|e| e <= now)
}
