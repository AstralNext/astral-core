//! 已安装服务登记（单机单服务，仅记录布局与参数）。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::health::default_data_root;
use super::SERVICE_REGISTRY_KEY;

/// 全局服务登记文件内容。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceRegistry {
    /// 安装根目录。
    #[serde(default)]
    pub install_root: Option<PathBuf>,
    /// 服务登记的 exe 路径。
    #[serde(default)]
    pub program: Option<PathBuf>,
    /// 服务代际（用于识别旧进程）。
    #[serde(default)]
    pub service_generation: Option<String>,
    /// 已安装实例。
    #[serde(default)]
    pub instances: Vec<InstalledInstance>,
}

/// 单个已安装实例。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledInstance {
    /// 实例名。
    pub name: String,
    /// gRPC 监听地址。
    pub listen: SocketAddr,
    /// 数据目录。
    pub data_dir: PathBuf,
    /// 是否用户级服务。
    #[serde(default)]
    pub user: bool,
}

fn registry_path() -> Result<PathBuf> {
    // 测试 / 便携：可覆盖登记文件路径
    if let Ok(p) = std::env::var("ASTRAL_SERVICE_REGISTRY") {
        let path = PathBuf::from(p);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        return Ok(path);
    }
    let root = default_data_root()?;
    std::fs::create_dir_all(&root)?;
    Ok(root.join("installed_services.json"))
}

fn canonicalize(path: &Path) -> PathBuf {
    let Ok(p) = std::fs::canonicalize(path) else {
        return path.to_path_buf();
    };
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            return PathBuf::from(stripped);
        }
    }
    p
}

/// 读取登记；文件不存在则返回空表。
pub fn load() -> Result<ServiceRegistry> {
    let path = registry_path()?;
    if !path.exists() {
        return Ok(ServiceRegistry::default());
    }
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("读取服务登记失败: {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("解析服务登记失败: {}", path.display()))
}

/// 写入登记（供清理 / 迁移模块使用）。
pub fn save_raw(reg: &ServiceRegistry) -> Result<()> {
    let path = registry_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(reg)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("写入服务登记失败: {}", path.display()))?;
    Ok(())
}

/// 安装成功后写入服务记录。
pub fn record_install(
    install_root: &Path,
    program: &Path,
    listen: SocketAddr,
    data_dir: &Path,
    user: bool,
) -> Result<()> {
    let reg = ServiceRegistry {
        install_root: Some(canonicalize(install_root)),
        program: Some(canonicalize(program)),
        service_generation: Some(super::SERVICE_GENERATION.to_string()),
        instances: vec![InstalledInstance {
            name: SERVICE_REGISTRY_KEY.to_string(),
            listen,
            data_dir: canonicalize(data_dir),
            user,
        }],
    };
    save_raw(&reg)
}

/// 更新后仅刷新程序路径。
pub fn record_program(install_root: &Path, program: &Path) -> Result<()> {
    let mut reg = load()?;
    reg.install_root = Some(canonicalize(install_root));
    reg.program = Some(canonicalize(program));
    reg.service_generation = Some(super::SERVICE_GENERATION.to_string());
    save_raw(&reg)
}

/// 卸载后清空登记。
pub fn record_uninstall() -> Result<()> {
    save_raw(&ServiceRegistry::default())
}
