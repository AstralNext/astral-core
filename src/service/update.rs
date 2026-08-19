//! 产品级自动更新：落入新版本目录 → 切换 current → 重启服务。

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};
use tracing::info;

use super::layout::{self, current_program};
use super::manage::{resolve_program, start, stop, ServiceActionOptions};
use super::registry;

/// 自动更新选项。
#[derive(Debug, Clone)]
pub struct UpdateOptions {
    /// 新二进制；缺省为当前进程。
    pub program: Option<PathBuf>,
    /// 版本号；缺省从二进制 `--version` 或 crate 版本推断。
    pub version: Option<String>,
    /// 安装根；缺省用登记中的 / 默认布局路径。
    pub install_root: Option<PathBuf>,
    /// 保留的版本数（含当前），默认 3。
    pub retain: usize,
    /// 切换后不启动。
    pub no_start: bool,
}

/// 回滚选项。
#[derive(Debug, Clone)]
pub struct RollbackOptions {
    /// 目标版本；缺省为除当前外最近修改的版本。
    pub version: Option<String>,
    /// 切换后不启动。
    pub no_start: bool,
}

/// 执行产品级更新（不覆盖正在运行的旧版本文件）。
pub fn update(opts: UpdateOptions) -> Result<()> {
    let reg = registry::load()?;
    if reg.instances.is_empty()
        && reg.install_root.is_none()
        && opts.install_root.is_none()
    {
        bail!("没有已安装服务记录，请先 service install（或传入 --install-root）");
    }

    let user = reg.instances.first().map(|i| i.user).unwrap_or(false);
    super::cleanup::prepare_install_or_update(user, false)?;
    super::recovery::begin_phase(
        super::recovery::MigrationPhase::StageNewVersion,
        opts.version.clone(),
        None,
    )?;

    let root = layout::resolve_install_root(
        opts.install_root
            .or_else(|| reg.install_root.clone()),
    )?;
    let source = resolve_program(opts.program)?;
    let version = layout::resolve_version(opts.version.as_deref(), &source)?;
    let instances = &reg.instances;

    info!(
        source = %source.display(),
        root = %root.display(),
        version = %version,
        "开始产品级更新"
    );

    let active = layout::read_active_version(&root).ok().flatten();
    let same_version = active.as_deref() == Some(version.as_str());

    // 同版本覆盖会锁住正在运行的 exe：先停再 stage
    if same_version {
        for inst in instances {
            let _ = stop(ServiceActionOptions { user: inst.user });
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    layout::stage_version(&root, &version, &source)?;

    if !same_version {
        for inst in instances {
            let _ = stop(ServiceActionOptions { user: inst.user });
        }
    }

    if let Err(e) = layout::switch_current(&root, &version) {
        // 切换失败：尽量把已停的实例拉起来，避免长期停机
        if !opts.no_start {
            for inst in instances {
                let _ = start(ServiceActionOptions { user: inst.user });
            }
        }
        return Err(e);
    }
    let program = current_program(&root);
    registry::record_active(&root, &version, &program)?;
    layout::prune_versions(&root, &version, opts.retain)?;

    if !opts.no_start {
        for inst in instances {
            start(ServiceActionOptions { user: inst.user })?;
        }
    }

    info!(
        version = %version,
        program = %program.display(),
        "自动更新完成"
    );
    super::recovery::begin_phase(super::recovery::MigrationPhase::Done, Some(version), None)?;
    let _ = super::recovery::clear_state();
    Ok(())
}

/// 将 `current` 切回旧版本并重启服务。
pub fn rollback(opts: RollbackOptions) -> Result<()> {
    let reg = registry::load()?;
    let root = layout::resolve_install_root(reg.install_root.clone())?;
    let active = reg
        .active_version
        .clone()
        .or_else(|| layout::read_active_version(&root).ok().flatten());

    let target = match opts.version {
        Some(v) => {
            layout::validate_version(&v)?;
            v
        }
        None => {
            let mut versions = layout::list_versions(&root)?;
            if let Some(a) = &active {
                versions.retain(|v| v != a);
            }
            versions.sort_by_key(|v| {
                layout::version_dir(&root, v)
                    .metadata()
                    .and_then(|m| m.modified())
                    .ok()
            });
            versions
                .pop()
                .ok_or_else(|| anyhow!("没有可回滚的旧版本"))?
        }
    };

    if active.as_deref() == Some(target.as_str()) {
        bail!("目标版本已是当前版本: {target}");
    }

    let instances = &reg.instances;
    info!(version = %target, "开始回滚");

    for inst in instances {
        let _ = stop(ServiceActionOptions { user: inst.user });
    }

    layout::switch_current(&root, &target)?;
    let program = current_program(&root);
    registry::record_active(&root, &target, &program)?;

    if !opts.no_start {
        for inst in instances {
            start(ServiceActionOptions { user: inst.user })?;
        }
    }

    info!(version = %target, "回滚完成");
    Ok(())
}

/// 打印已安装版本列表。
pub fn list_versions_report() -> Result<String> {
    let reg = registry::load()?;
    let root = layout::resolve_install_root(reg.install_root.clone())?;
    let active = reg
        .active_version
        .clone()
        .or_else(|| layout::read_active_version(&root).ok().flatten());
    let versions = layout::list_versions(&root)?;
    let mut lines = vec![format!("install_root={}", root.display())];
    if versions.is_empty() {
        lines.push("(no versions)".into());
    } else {
        for v in versions {
            let mark = if active.as_deref() == Some(v.as_str()) {
                "*"
            } else {
                " "
            };
            lines.push(format!("{mark} {v}"));
        }
    }
    Ok(lines.join("\n"))
}
