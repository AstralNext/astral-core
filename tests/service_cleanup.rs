//! 服务清理 / 体检 / 迁移单元测试。

use std::fs;
use std::sync::Mutex;

use astral_core::service::{
    begin_phase, clear_state, health_report_json, inspect_health, load_state,
    load_service_registry, migrate_legacy_data_if_needed, normalize_registry_generation,
    record_install, remove_legacy_data_dir_if_safe, save_raw, MigrationPhase, SERVICE_GENERATION,
};
use tempfile::TempDir;

static DATA_DIR_TEST_LOCK: Mutex<()> = Mutex::new(());

struct RegistryGuard {
    _dir: TempDir,
    prev: Option<String>,
}

impl RegistryGuard {
    fn new() -> Self {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("installed_services.json");
        let prev = std::env::var("ASTRAL_SERVICE_REGISTRY").ok();
        std::env::set_var("ASTRAL_SERVICE_REGISTRY", &path);
        Self { _dir: dir, prev }
    }
}

impl Drop for RegistryGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var("ASTRAL_SERVICE_REGISTRY", v),
            None => std::env::remove_var("ASTRAL_SERVICE_REGISTRY"),
        }
    }
}

#[test]
fn health_report_json_is_valid_json() {
    let text = health_report_json(false).expect("doctor json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parse json");
    assert_eq!(value["service_name"], "dev.astral.core");
    assert_eq!(value["service_generation"], SERVICE_GENERATION);
    assert!(value["issues"].is_array());
}

#[test]
fn inspect_health_reports_legacy_dir_when_present() {
    let report = inspect_health(false).expect("health");
    if report.legacy_data_dir_exists {
        assert!(report.issues.iter().any(|i| i.contains("instances")));
    }
}

#[test]
fn migrate_legacy_data_skips_when_node_id_exists() {
    let _lock = DATA_DIR_TEST_LOCK.lock().unwrap();
    let root = TempDir::new().unwrap();
    let prev = std::env::var("ASTRAL_CORE_DATA_DIR").ok();
    std::env::set_var("ASTRAL_CORE_DATA_DIR", root.path());

    fs::create_dir_all(root.path().join("instances/default")).unwrap();
    fs::write(root.path().join("node_id"), b"existing\n").unwrap();

    assert!(!migrate_legacy_data_if_needed(false).unwrap());

    match prev {
        Some(v) => std::env::set_var("ASTRAL_CORE_DATA_DIR", v),
        None => std::env::remove_var("ASTRAL_CORE_DATA_DIR"),
    }
}

#[test]
fn remove_legacy_data_dir_when_node_id_at_root() {
    let _lock = DATA_DIR_TEST_LOCK.lock().unwrap();
    let root = TempDir::new().unwrap();
    let prev = std::env::var("ASTRAL_CORE_DATA_DIR").ok();
    std::env::set_var("ASTRAL_CORE_DATA_DIR", root.path());

    fs::create_dir_all(root.path().join("instances/default")).unwrap();
    fs::write(root.path().join("node_id"), b"existing\n").unwrap();

    assert!(remove_legacy_data_dir_if_safe().unwrap());
    assert!(!root.path().join("instances/default").exists());

    match prev {
        Some(v) => std::env::set_var("ASTRAL_CORE_DATA_DIR", v),
        None => std::env::remove_var("ASTRAL_CORE_DATA_DIR"),
    }
}

#[test]
fn normalize_registry_generation_updates_old_value() {
    let _reg = RegistryGuard::new();
    let tmp = TempDir::new().unwrap();
    let install = tmp.path().join("app");
    fs::create_dir_all(&install).unwrap();
    let data = tmp.path().join("data");
    fs::create_dir_all(&data).unwrap();
    let prog = install.join("current").join("astral-core.exe");
    fs::create_dir_all(prog.parent().unwrap()).unwrap();
    fs::write(&prog, b"bin").unwrap();

    record_install(
        &install,
        "0.1.0",
        &prog,
        "127.0.0.1:50051".parse().unwrap(),
        &data,
        false,
    )
    .unwrap();

    let mut reg = load_service_registry().unwrap();
    reg.service_generation = Some("core-v1".into());
    save_raw(&reg).unwrap();

    assert!(normalize_registry_generation().unwrap());
    let reg = load_service_registry().unwrap();
    assert_eq!(reg.service_generation.as_deref(), Some(SERVICE_GENERATION));
}

#[test]
fn migration_state_roundtrip() {
    let _lock = DATA_DIR_TEST_LOCK.lock().unwrap();
    let root = TempDir::new().unwrap();
    let prev = std::env::var("ASTRAL_CORE_DATA_DIR").ok();
    std::env::set_var("ASTRAL_CORE_DATA_DIR", root.path());

    begin_phase(MigrationPhase::Preflight, None, Some("test".into())).unwrap();
    let state = load_state().unwrap().expect("state");
    assert_eq!(state.phase, MigrationPhase::Preflight);
    clear_state().unwrap();
    assert!(load_state().unwrap().is_none());

    match prev {
        Some(v) => std::env::set_var("ASTRAL_CORE_DATA_DIR", v),
        None => std::env::remove_var("ASTRAL_CORE_DATA_DIR"),
    }
}
