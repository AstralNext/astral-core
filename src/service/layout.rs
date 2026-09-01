//! 固定安装布局：一个目录一个 exe，无版本子目录、无链接。
//!
//! ```text
//! {install_root}/astral-core[.exe]   ← 服务登记的唯一程序路径
//! ```
//!
//! 安装 = 复制 exe 到固定路径；更新 = 停服务 → 覆盖 exe → 启服务。

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
#[cfg(not(windows))]
use directories::ProjectDirs;
use tracing::info;

use super::manage::{dunce_canonicalize, resolve_program};

/// 可执行文件名。
pub fn binary_name() -> &'static str {
    if cfg!(windows) {
        "astral-core.exe"
    } else {
        "astral-core"
    }
}

/// 默认安装根目录（与 GUI 对齐）。
pub fn default_install_root() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let root = std::env::var("PROGRAMFILES")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("C:\\Program Files"))
            .join("nextAstral");
        Ok(root)
    }
    #[cfg(not(windows))]
    {
        let dirs = ProjectDirs::from("dev", "Astral", "astral-core")
            .ok_or_else(|| anyhow::anyhow!("无法解析平台数据目录"))?;
        Ok(dirs.data_local_dir().join("app"))
    }
}

/// 解析安装根；相对路径基于当前工作目录。
pub fn resolve_install_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let root = match explicit {
        Some(p) if p.as_os_str().is_empty() => anyhow::bail!("install-root 不能为空"),
        Some(p) if p.is_absolute() => p,
        Some(p) => std::env::current_dir()?.join(p),
        None => default_install_root()?,
    };
    fs::create_dir_all(&root).with_context(|| format!("创建安装根目录失败: {}", root.display()))?;
    Ok(dunce_canonicalize(&root).unwrap_or(root))
}

/// `{root}/astral-core[.exe]` —— 服务登记的唯一程序路径。
pub fn program_path(root: &Path) -> PathBuf {
    root.join(binary_name())
}

/// 把源二进制（含 sidecar DLL）复制到固定路径，返回 exe 路径。
pub fn stage_binary(root: &Path, source: &Path) -> Result<PathBuf> {
    let source = resolve_program(Some(source.to_path_buf()))?;
    fs::create_dir_all(root).with_context(|| format!("创建安装目录失败: {}", root.display()))?;
    let dest = program_path(root);
    if !same_path(&source, &dest) {
        // 目标 exe 可能正在运行：调用方负责先停服务；这里删不掉就报错
        if dest.exists() {
            let _ = fs::remove_file(&dest);
        }
        fs::copy(&source, &dest).with_context(|| {
            format!(
                "复制二进制失败: {} -> {}",
                source.display(),
                dest.display()
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&dest)?.permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&dest, perms)?;
        }
    }
    copy_sidecars(&source, root)?;
    info!(dest = %dest.display(), "内核已就位");
    Ok(dest)
}

/// Windows 上把 `wintun.dll` / `Packet.dll` 拷到 exe 同级目录。
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

/// 清理旧版布局残留：`current` 链接与版本子目录。尽力删除，失败不报错。
pub fn cleanup_legacy_layout(root: &Path) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy().into_owned();
        if name == "current" || name.parse::<f64>().map(|v| v > 0.0).unwrap_or(false) {
            let p = entry.path();
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let _ = fs::remove_dir_all(&p);
            }
            info!(path = %p.display(), "清理旧版布局残留");
        }
    }
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
