//! 跨平台系统服务封装（systemd / launchd / Windows SCM）。
//!
//! 本机只装一个服务：`dev.astral.core-default`。

mod layout;
mod manage;
mod registry;
mod run;
mod update;

#[cfg(windows)]
mod windows_host;

/// 固定服务实例名（与历史安装兼容）。
pub const SERVICE_INSTANCE_NAME: &str = "default";

pub use layout::{
    binary_name, current_program, list_versions, read_active_version, resolve_install_root,
    stage_version, switch_current, validate_version, version_dir, version_program,
};
pub use manage::{
    install, service_label, start, status, status_label, stop, uninstall, InstallOptions,
    ServiceActionOptions,
};
pub use registry::{load as load_service_registry, record_install, record_uninstall, InstalledInstance};
pub use run::{bootstrap_runtime, run_foreground, shutdown_signal, RunParams};
pub use update::{list_versions_report, rollback, update, RollbackOptions, UpdateOptions};

#[cfg(windows)]
pub use windows_host::run_as_windows_service;
