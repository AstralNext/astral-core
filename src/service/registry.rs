//! 已安装服务实例登记（供自动更新批量停启）。

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

use super::SERVICE_REGISTRY_KEY;

/// 全局服务登记文件内容。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServiceRegistry {
    /// 并排版本安装根目录（含 `current` 与各版本目录）。
    #[serde(default)]
    pub install_root: Option<PathBuf>,
    /// 当前激活的版本号。
    #[serde(default)]
    pub active_version: Option<String>,
    /// 服务登记的稳定 exe：`{install_root}/current/astral-core[.exe]`。
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
    let dirs = ProjectDirs::from("dev", "Astral", "astral-core")
        .ok_or_else(|| anyhow!("无法解析平台数据目录"))?;
    let root = dirs.data_dir();
    std::fs::create_dir_all(root)?;
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

fn same_install_root(a: &Path, b: &Path) -> bool {
    canonicalize(a) == canonicalize(b)
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

fn save(reg: &ServiceRegistry) -> Result<()> {
    save_raw(reg)
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

/// 安装成功后写入服务记录，并记录布局信息。
pub fn record_install(
    install_root: &Path,
    active_version: &str,
    program: &Path,
    listen: SocketAddr,
    data_dir: &Path,
    user: bool,
) -> Result<()> {
    let install_root = canonicalize(install_root);
    let program = canonicalize(program);
    let data_dir = canonicalize(data_dir);
    let mut reg = load()?;
    if let Some(existing) = reg.install_root.as_ref() {
        if !same_install_root(existing, &install_root) {
            return Err(anyhow!(
                "已登记安装根为 {}，不能再安装到 {}（单登记文件仅支持一个 install_root）",
                existing.display(),
                install_root.display()
            ));
        }
    }
    reg.install_root = Some(install_root);
    reg.active_version = Some(active_version.to_string());
    reg.program = Some(program);
    reg.service_generation = Some(super::SERVICE_GENERATION.to_string());
    if let Some(existing) = reg.instances.first_mut() {
        existing.listen = listen;
        existing.data_dir = data_dir;
        existing.user = user;
    } else {
        reg.instances.push(InstalledInstance {
            name: SERVICE_REGISTRY_KEY.to_string(),
            listen,
            data_dir,
            user,
        });
    }
    save(&reg)
}

/// 卸载后移除服务记录；若无记录则清空布局字段。
pub fn record_uninstall(user: bool) -> Result<()> {
    let mut reg = load()?;
    reg.instances
        .retain(|i| !(i.name == SERVICE_REGISTRY_KEY && i.user == user));
    if reg.instances.is_empty() {
        reg.program = None;
        reg.install_root = None;
        reg.active_version = None;
        reg.service_generation = None;
    }
    save(&reg)
}

/// 切换版本后更新登记。
pub fn record_active(install_root: &Path, active_version: &str, program: &Path) -> Result<()> {
    let install_root = canonicalize(install_root);
    let mut reg = load()?;
    if let Some(existing) = reg.install_root.as_ref() {
        if !same_install_root(existing, &install_root) {
            return Err(anyhow!(
                "更新目标安装根 {} 与登记 {} 不一致",
                install_root.display(),
                existing.display()
            ));
        }
    }
    reg.install_root = Some(install_root);
    reg.active_version = Some(active_version.to_string());
    reg.program = Some(canonicalize(program));
    save(&reg)
}
