//! 服务更新：停服务 → 覆盖固定路径 exe → 启服务。

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use tracing::info;

use super::layout;
use super::manage::{resolve_program, start, stop, ServiceActionOptions};

/// 更新选项。
#[derive(Debug, Clone)]
pub struct UpdateOptions {
    /// 新二进制；缺省为当前进程。
    pub program: Option<PathBuf>,
    /// 安装根；缺省用登记值 / 默认固定目录。
    pub install_root: Option<PathBuf>,
    /// 覆盖后不启动。
    pub no_start: bool,
}

/// 执行更新：停服务 → 覆盖 exe → 启服务。
pub fn update(opts: UpdateOptions) -> Result<()> {
    let reg = super::registry::load().ok();
    let user = reg
        .as_ref()
        .and_then(|r| r.instances.first())
        .map(|i| i.user)
        .unwrap_or(false);
    let root = layout::resolve_install_root(opts.install_root.or_else(|| {
        reg.as_ref()
            .and_then(|r| r.install_root.clone())
            .filter(|p| p.exists())
    }))?;

    let source = resolve_program(opts.program)?;
    info!(
        source = %source.display(),
        root = %root.display(),
        "开始更新内核"
    );

    // 覆盖正在运行的 exe 会失败：先停服务
    let _ = stop(ServiceActionOptions { user });
    thread::sleep(Duration::from_millis(500));

    let program = layout::stage_binary(&root, &source)?;
    super::registry::record_program(&root, &program)?;

    if !opts.no_start {
        start(ServiceActionOptions { user })?;
    }

    info!(program = %program.display(), "更新完成");
    Ok(())
}
