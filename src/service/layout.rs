//! 产品级安装布局：并排版本目录 + `current` 入口。
//!
//! ```text
//! {install_root}/
//!   current/          → 目录结/符号链接，指向某个版本目录
//!   0.1.0/astral-core[.exe]
//!   0.1.1/astral-core[.exe]
//! ```
//!
//! 系统服务始终登记 `{install_root}/current/astral-core[.exe]`。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};
use directories::ProjectDirs;
use tracing::info;

use super::manage::{dunce_canonicalize, resolve_program};

/// 可执行文件在版本目录中的文件名。
pub fn binary_name() -> &'static str {
    if cfg!(windows) {
        "astral-core.exe"
    } else {
        "astral-core"
    }
}

/// 默认安装根目录（与 GUI 对齐）。
pub fn default_install_root() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("dev", "Astral", "astral-core")
        .ok_or_else(|| anyhow!("无法解析平台数据目录"))?;
    #[cfg(windows)]
    {
        let local = std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| dirs.data_local_dir().to_path_buf());
        Ok(local
            .join("Astral")
            .join("astral-core")
            .join("data")
            .join("app"))
    }
    #[cfg(not(windows))]
    {
        Ok(dirs.data_local_dir().join("app"))
    }
}

/// 解析安装根；相对路径基于当前工作目录。
pub fn resolve_install_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let root = match explicit {
        Some(p) if p.as_os_str().is_empty() => bail!("install-root 不能为空"),
        Some(p) if p.is_absolute() => p,
        Some(p) => std::env::current_dir()?.join(p),
        None => default_install_root()?,
    };
    fs::create_dir_all(&root).with_context(|| format!("创建安装根目录失败: {}", root.display()))?;
    Ok(dunce_canonicalize(&root).unwrap_or(root))
}

/// `{root}/current`
pub fn current_link(root: &Path) -> PathBuf {
    root.join("current")
}

/// `{root}/current/astral-core[.exe]` —— 服务登记的稳定路径。
pub fn current_program(root: &Path) -> PathBuf {
    current_link(root).join(binary_name())
}

/// `{root}/{version}`
pub fn version_dir(root: &Path, version: &str) -> PathBuf {
    root.join(version)
}

/// `{root}/{version}/astral-core[.exe]`
pub fn version_program(root: &Path, version: &str) -> PathBuf {
    version_dir(root, version).join(binary_name())
}

/// 校验版本目录名（禁止路径穿越）。
pub fn validate_version(version: &str) -> Result<()> {
    if version.is_empty() || version.len() > 64 {
        bail!("版本号长度须为 1..=64");
    }
    if version == "current" {
        bail!("版本号不能为 current");
    }
    if version.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) {
        bail!("版本号含非法字符: {version}");
    }
    if version == "." || version == ".." || version.starts_with('.') {
        bail!("版本号非法: {version}");
    }
    Ok(())
}

/// 推断版本：显式 > 运行 `program --version` > 当前 crate 版本。
pub fn resolve_version(explicit: Option<&str>, program: &Path) -> Result<String> {
    if let Some(v) = explicit {
        validate_version(v)?;
        return Ok(v.to_string());
    }
    if let Some(v) = probe_program_version(program) {
        validate_version(&v)?;
        return Ok(v);
    }
    let v = env!("CARGO_PKG_VERSION").to_string();
    validate_version(&v)?;
    Ok(v)
}

fn probe_program_version(program: &Path) -> Option<String> {
    let output = Command::new(program).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    // e.g. "astral-core 0.1.0"
    text.split_whitespace().nth(1).map(|s| s.trim().to_string())
}

/// 把 `source` 安装进 `{root}/{version}/`，返回版本内 exe 路径。
pub fn stage_version(root: &Path, version: &str, source: &Path) -> Result<PathBuf> {
    validate_version(version)?;
    let source = resolve_program(Some(source.to_path_buf()))?;
    let dir = version_dir(root, version);
    fs::create_dir_all(&dir).with_context(|| format!("创建版本目录失败: {}", dir.display()))?;
    let dest = version_program(root, version);
    if same_path(&source, &dest) {
        copy_sidecars(&source, &dir)?;
        return Ok(dest);
    }
    fs::copy(&source, &dest)
        .with_context(|| format!("复制二进制失败: {} -> {}", source.display(), dest.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dest)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest, perms)?;
    }
    copy_sidecars(&source, &dir)?;
    info!(
        version,
        dest = %dest.display(),
        "版本已落入安装布局"
    );
    Ok(dest)
}

/// Windows 上把 `wintun.dll` / `Packet.dll` 拷到版本目录（与 exe 同级）。
pub fn copy_sidecars(source_program: &Path, dest_dir: &Path) -> Result<()> {
    #[cfg(not(windows))]
    {
        let _ = (source_program, dest_dir);
        return Ok(());
    }
    #[cfg(windows)]
    {
        const NAMES: &[&str] = &["wintun.dll", "Packet.dll"];
        let Some(src_dir) = source_program.parent() else {
            return Ok(());
        };
        fs::create_dir_all(dest_dir)
            .with_context(|| format!("创建 sidecar 目录失败: {}", dest_dir.display()))?;
        for name in NAMES {
            let from = src_dir.join(name);
            if !from.is_file() {
                continue;
            }
            let to = dest_dir.join(name);
            if same_path(&from, &to) {
                continue;
            }
            fs::copy(&from, &to).with_context(|| {
                format!("复制 sidecar 失败: {} -> {}", from.display(), to.display())
            })?;
            info!(sidecar = name, dest = %to.display(), "已复制运行时 DLL");
        }
        Ok(())
    }
}

/// 将 `current` 指向指定版本目录（Windows: junction；Unix: symlink）。
/// 失败时尽量恢复到切换前的版本。
pub fn switch_current(root: &Path, version: &str) -> Result<()> {
    validate_version(version)?;
    let prog = version_program(root, version);
    if !prog.exists() {
        bail!("版本不存在或缺少二进制: {}", prog.display());
    }
    let previous = read_active_version(root).ok().flatten();
    let link = current_link(root);
    remove_current_link(&link)?;

    if let Err(e) = create_current_link(root, version) {
        if let Some(prev) = previous.as_deref() {
            if version_program(root, prev).exists() {
                let _ = create_current_link(root, prev);
            }
        }
        return Err(e);
    }

    info!(
        current = %link.display(),
        version,
        "已切换 current"
    );
    Ok(())
}

fn create_current_link(root: &Path, version: &str) -> Result<()> {
    let link = current_link(root);
    #[cfg(windows)]
    {
        let status = Command::new("cmd")
            .current_dir(root)
            .args(["/C", "mklink", "/J", "current", version])
            .status()
            .context("执行 mklink /J 失败")?;
        if !status.success() {
            bail!("创建目录结失败: current -> {version} (exit={status})");
        }
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(version, &link)
            .with_context(|| format!("创建符号链接失败: {} -> {version}", link.display()))?;
    }
    let _ = link;
    Ok(())
}

fn remove_current_link(link: &Path) -> Result<()> {
    if !link.exists() && !is_symlink_or_junction(link) {
        return Ok(());
    }

    #[cfg(windows)]
    {
        // 目录结必须用 rmdir 去掉链接本身；remove_dir_all 可能误伤目标。
        if is_symlink_or_junction(link) {
            let parent = link.parent().ok_or_else(|| anyhow!("current 无父目录"))?;
            let name = link
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or_else(|| anyhow!("非法 current 名"))?;
            let status = Command::new("cmd")
                .current_dir(parent)
                .args(["/C", "rmdir", name])
                .status()
                .context("执行 rmdir 失败")?;
            if !status.success() && is_symlink_or_junction(link) {
                bail!("移除 current 目录结失败: {}", link.display());
            }
            return Ok(());
        }
        if link.is_dir() {
            fs::remove_dir_all(link)
                .with_context(|| format!("移除残缺 current 目录失败: {}", link.display()))?;
        } else if link.is_file() {
            fs::remove_file(link)
                .with_context(|| format!("移除 current 失败: {}", link.display()))?;
        }
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        let meta = fs::symlink_metadata(link);
        match meta {
            Ok(m) if m.file_type().is_symlink() => {
                fs::remove_file(link)
                    .or_else(|_| fs::remove_dir(link))
                    .with_context(|| format!("移除 current 失败: {}", link.display()))?;
            }
            Ok(m) if m.is_dir() => {
                fs::remove_dir_all(link)
                    .with_context(|| format!("移除 current 失败: {}", link.display()))?;
            }
            Ok(_) => {
                fs::remove_file(link)
                    .with_context(|| format!("移除 current 失败: {}", link.display()))?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(e).with_context(|| format!("移除 current 失败: {}", link.display()));
            }
        }
        Ok(())
    }
}

fn is_symlink_or_junction(path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        return meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0;
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// 列出安装根下的版本目录名（排除 current）。
pub fn list_versions(root: &Path) -> Result<Vec<String>> {
    let mut versions = Vec::new();
    if !root.exists() {
        return Ok(versions);
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "current" {
            continue;
        }
        let Ok(()) = validate_version(&name) else {
            continue;
        };
        if version_program(root, &name).exists() {
            versions.push(name.into_owned());
        }
    }
    versions.sort();
    Ok(versions)
}

/// 读取 `current` 当前指向的版本目录名。
pub fn read_active_version(root: &Path) -> Result<Option<String>> {
    let link = current_link(root);
    if !link.exists() && !is_symlink_or_junction(&link) {
        return Ok(None);
    }
    let resolved = dunce_canonicalize(&link).unwrap_or(link);
    let name = resolved
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| validate_version(s).is_ok());
    Ok(name)
}

/// 保留最近 `retain` 个版本（含当前），删除更旧的。
pub fn prune_versions(root: &Path, active: &str, retain: usize) -> Result<()> {
    if retain == 0 {
        return Ok(());
    }
    let mut versions = list_versions(root)?;
    // 简单按名字排序不够 semver；保留 active + 其余按修改时间
    versions.sort_by_key(|v| {
        version_dir(root, v)
            .metadata()
            .and_then(|m| m.modified())
            .ok()
    });
    versions.retain(|v| v != active);
    // 最旧在前；保留 retain-1 个旧版
    let keep_old = retain.saturating_sub(1);
    while versions.len() > keep_old {
        let old = versions.remove(0);
        let dir = version_dir(root, &old);
        info!(version = %old, path = %dir.display(), "清理旧版本");
        let _ = fs::remove_dir_all(&dir);
    }
    Ok(())
}

fn same_path(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (dunce_canonicalize(a), dunce_canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}
