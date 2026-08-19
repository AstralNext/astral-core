//! 跨平台系统服务封装（systemd / launchd / Windows SCM）。
//!
//! 本机只装一个服务：`dev.astral.core`。

mod cleanup;
mod health;
mod layout;
mod manage;
mod recovery;
mod registry;
mod run;
mod update;

#[cfg(windows)]
mod windows_host;

/// 系统服务限定名（Windows SCM / systemd / launchd）。
pub const SERVICE_QUALIFIED_NAME: &str = "dev.astral.core";

/// 旧版服务名（迁移时需卸载）。
pub const LEGACY_SERVICE_QUALIFIED_NAME: &str = "dev.astral.core-default";

/// 服务代际标记（进程参数 / 登记文件）。
pub const SERVICE_GENERATION: &str = "core-v2";

/// 服务登记文件中的固定键（本机仅一条记录）。
pub(crate) const SERVICE_REGISTRY_KEY: &str = "core";

pub use cleanup::{
    cleanup_after_uninstall, migrate_legacy_data_if_needed, normalize_registry_generation,
    prepare_install_or_update, remove_legacy_data_dir_if_safe, repair_environment, RepairOptions,
    RepairReport,
};
pub use health::{health_report_json, inspect_health, ServiceHealthReport};
pub use layout::{
    binary_name, current_program, list_versions, read_active_version, resolve_install_root,
    stage_version, switch_current, validate_version, version_dir, version_program,
};
pub use manage::{
    install, service_label, start, status, status_label, stop, uninstall, InstallOptions,
    ServiceActionOptions, UninstallOptions,
};
pub use recovery::{
    begin_phase, clear_state, load_state, MigrationPhase, MigrationState,
};
pub use registry::{
    load as load_service_registry, record_install, record_uninstall, save_raw, InstalledInstance,
};
pub use run::{bootstrap_runtime, run_foreground, shutdown_signal, RunParams};
pub use update::{list_versions_report, rollback, update, RollbackOptions, UpdateOptions};

#[cfg(windows)]
pub use windows_host::run_as_windows_service;
