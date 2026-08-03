//! 设备凭证落盘（重连用）。

use std::fs;
use std::path::Path;

use crate::error::{CoreError, CoreResult};

/// 设备凭证文件名（位于 data-dir）。
pub const DEVICE_CREDENTIAL_FILE: &str = "agent_device_credential";

/// 读取已保存的设备凭证。
pub fn load_device_credential(data_dir: &Path) -> CoreResult<Option<String>> {
    let path = data_dir.join(DEVICE_CREDENTIAL_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let s = fs::read_to_string(path)?.trim().to_string();
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

/// 写入设备凭证。
pub fn save_device_credential(data_dir: &Path, credential: &str) -> CoreResult<()> {
    if credential.is_empty() {
        return Err(CoreError::InvalidArgument("空设备凭证".into()));
    }
    let path = data_dir.join(DEVICE_CREDENTIAL_FILE);
    fs::write(path, credential.as_bytes())?;
    Ok(())
}
