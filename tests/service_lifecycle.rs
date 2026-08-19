//! 服务布局 / 登记 / 更新 / 回滚测试（不依赖系统服务权限）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use astral_core::service::{
    self, binary_name, current_program, list_versions, list_versions_report, load_service_registry,
    read_active_version, record_install, record_uninstall, service_label, stage_version,
    switch_current, update, validate_version, version_dir, version_program, RollbackOptions,
    UpdateOptions,
};
use tempfile::TempDir;

/// 串行化：登记文件靠环境变量，避免并行测试互相覆盖。
static REGISTRY_LOCK: Mutex<()> = Mutex::new(());

struct RegistryGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    _dir: TempDir,
    prev: Option<String>,
}

impl RegistryGuard {
    fn new() -> Self {
        let lock = REGISTRY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("installed_services.json");
        let prev = std::env::var("ASTRAL_SERVICE_REGISTRY").ok();
        std::env::set_var("ASTRAL_SERVICE_REGISTRY", &path);
        Self {
            _lock: lock,
            _dir: dir,
            prev,
        }
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

fn write_fake_bin(path: &Path, marker: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, format!("fake-bin:{marker}")).unwrap();
}

fn read_marker(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn validate_version_and_service_label() {
    assert!(validate_version("0.1.0").is_ok());
    assert!(validate_version("1.2.3-beta").is_ok());
    assert!(validate_version("current").is_err());
    assert!(validate_version("../x").is_err());
    assert!(validate_version("").is_err());

    let label = service_label().unwrap();
    assert_eq!(label.to_qualified_name(), "dev.astral.core");
    assert_eq!(label.to_script_name(), "astral-core");
}

#[test]
fn layout_stage_switch_list_and_stable_current_path() {
    let root = TempDir::new().unwrap();
    let root = root.path();
    let src1 = root.join("download").join("v1").join(binary_name());
    let src2 = root.join("download").join("v2").join(binary_name());
    write_fake_bin(&src1, "v1");
    write_fake_bin(&src2, "v2");

    let install = root.join("app");
    fs::create_dir_all(&install).unwrap();

    stage_version(&install, "0.1.0", &src1).unwrap();
    switch_current(&install, "0.1.0").unwrap();
    assert_eq!(
        read_active_version(&install).unwrap().as_deref(),
        Some("0.1.0")
    );
    assert_eq!(read_marker(&current_program(&install)), "fake-bin:v1");
    assert_eq!(
        read_marker(&version_program(&install, "0.1.0")),
        "fake-bin:v1"
    );

    stage_version(&install, "0.1.1", &src2).unwrap();
    switch_current(&install, "0.1.1").unwrap();
    assert_eq!(
        read_active_version(&install).unwrap().as_deref(),
        Some("0.1.1")
    );
    // 稳定入口仍是 current/，内容已切到 v2；旧版文件仍在
    assert_eq!(read_marker(&current_program(&install)), "fake-bin:v2");
    assert_eq!(
        read_marker(&version_program(&install, "0.1.0")),
        "fake-bin:v1"
    );

    let vers = list_versions(&install).unwrap();
    assert!(vers.contains(&"0.1.0".into()));
    assert!(vers.contains(&"0.1.1".into()));
}

#[cfg(windows)]
#[test]
fn stage_version_copies_wintun_sidecar() {
    let root = TempDir::new().unwrap();
    let src_dir = root.path().join("download");
    fs::create_dir_all(&src_dir).unwrap();
    let src = src_dir.join(binary_name());
    write_fake_bin(&src, "core");
    fs::write(src_dir.join("wintun.dll"), b"signed-wintun").unwrap();
    fs::write(src_dir.join("Packet.dll"), b"packet").unwrap();

    let install = root.path().join("app");
    fs::create_dir_all(&install).unwrap();
    stage_version(&install, "0.1.0", &src).unwrap();

    assert_eq!(
        fs::read(version_dir(&install, "0.1.0").join("wintun.dll")).unwrap(),
        b"signed-wintun"
    );
    assert_eq!(
        fs::read(version_dir(&install, "0.1.0").join("Packet.dll")).unwrap(),
        b"packet"
    );
}

#[test]
fn update_and_rollback_with_retain() {
    let _reg = RegistryGuard::new();
    let tmp = TempDir::new().unwrap();
    let install = tmp.path().join("app");
    fs::create_dir_all(&install).unwrap();

    let mk_src = |name: &str| -> PathBuf {
        let p = tmp.path().join("src").join(name).join(binary_name());
        write_fake_bin(&p, name);
        p
    };
    let s1 = mk_src("a");
    let s2 = mk_src("b");
    let s3 = mk_src("c");
    let s4 = mk_src("d");

    // 首次：无登记，但带 install-root 应可落入布局
    update(UpdateOptions {
        program: Some(s1.clone()),
        version: Some("1.0.0".into()),
        install_root: Some(install.clone()),
        retain: 3,
        no_start: true,
    })
    .expect("bootstrap update");

    let reg = load_service_registry().unwrap();
    assert_eq!(reg.active_version.as_deref(), Some("1.0.0"));
    assert!(reg.install_root.is_some());
    assert_eq!(read_marker(&current_program(&install)), "fake-bin:a");

    update(UpdateOptions {
        program: Some(s2),
        version: Some("1.0.1".into()),
        install_root: Some(install.clone()),
        retain: 3,
        no_start: true,
    })
    .unwrap();
    update(UpdateOptions {
        program: Some(s3),
        version: Some("1.0.2".into()),
        install_root: Some(install.clone()),
        retain: 3,
        no_start: true,
    })
    .unwrap();
    // 第 4 个版本，retain=3 应清掉最旧
    update(UpdateOptions {
        program: Some(s4),
        version: Some("1.0.3".into()),
        install_root: Some(install.clone()),
        retain: 3,
        no_start: true,
    })
    .unwrap();

    let vers = list_versions(&install).unwrap();
    assert!(
        !vers.contains(&"1.0.0".into()),
        "最旧版应被 prune: {vers:?}"
    );
    assert!(vers.contains(&"1.0.3".into()));
    assert_eq!(
        read_active_version(&install).unwrap().as_deref(),
        Some("1.0.3")
    );
    assert_eq!(read_marker(&current_program(&install)), "fake-bin:d");

    // 回滚到上一版（按 mtime，应为 1.0.2）
    service::rollback(RollbackOptions {
        version: None,
        no_start: true,
    })
    .unwrap();
    let active = read_active_version(&install).unwrap().unwrap();
    assert_ne!(active, "1.0.3");
    assert!(
        ["1.0.1", "1.0.2"].contains(&active.as_str()),
        "unexpected {active}"
    );

    // 显式回滚到仍存在的版本
    if version_program(&install, "1.0.1").exists() {
        service::rollback(RollbackOptions {
            version: Some("1.0.1".into()),
            no_start: true,
        })
        .unwrap();
        assert_eq!(
            read_active_version(&install).unwrap().as_deref(),
            Some("1.0.1")
        );
        assert_eq!(read_marker(&current_program(&install)), "fake-bin:b");
    }

    let report = list_versions_report().unwrap();
    assert!(report.contains("install_root="));
    assert!(report.contains('*'));
}

#[test]
fn registry_record_install_and_uninstall() {
    let _reg = RegistryGuard::new();
    let tmp = TempDir::new().unwrap();
    let install = tmp.path().join("app");
    let data = tmp.path().join("data");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(&data).unwrap();
    let prog = install.join("current").join(binary_name());
    write_fake_bin(&prog, "x");

    record_install(
        &install,
        "0.1.0",
        &prog,
        "127.0.0.1:50051".parse().unwrap(),
        &data,
        false,
    )
    .unwrap();

    let reg = load_service_registry().unwrap();
    assert_eq!(reg.instances.len(), 1);
    assert_eq!(reg.instances[0].name, "core");
    assert_eq!(reg.active_version.as_deref(), Some("0.1.0"));

    record_uninstall(false).unwrap();
    let reg = load_service_registry().unwrap();
    assert!(reg.instances.is_empty());
    assert!(reg.install_root.is_none());
    assert!(reg.active_version.is_none());
}

#[test]
fn registry_rejects_conflicting_install_root() {
    let _reg = RegistryGuard::new();
    let tmp = TempDir::new().unwrap();
    let install_a = tmp.path().join("app-a");
    let install_b = tmp.path().join("app-b");
    let data = tmp.path().join("data");
    fs::create_dir_all(&install_a).unwrap();
    fs::create_dir_all(&install_b).unwrap();
    fs::create_dir_all(&data).unwrap();
    let prog_a = install_a.join("current").join(binary_name());
    write_fake_bin(&prog_a, "x");

    record_install(
        &install_a,
        "0.1.0",
        &prog_a,
        "127.0.0.1:50051".parse().unwrap(),
        &data,
        false,
    )
    .unwrap();

    let prog_b = install_b.join("current").join(binary_name());
    write_fake_bin(&prog_b, "y");
    let err = record_install(
        &install_b,
        "0.1.0",
        &prog_b,
        "127.0.0.1:50052".parse().unwrap(),
        &data,
        false,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("install_root"),
        "unexpected: {err}"
    );
}

#[test]
fn registry_updates_existing_service_record() {
    let _reg = RegistryGuard::new();
    let tmp = TempDir::new().unwrap();
    let install = tmp.path().join("app");
    fs::create_dir_all(&install).unwrap();
    let src = tmp.path().join(binary_name());
    write_fake_bin(&src, "z");
    let data = tmp.path().join("d");
    fs::create_dir_all(&data).unwrap();

    record_install(
        &install,
        "0.1.0",
        &src,
        "127.0.0.1:50051".parse().unwrap(),
        &data,
        false,
    )
    .unwrap();

    record_install(
        &install,
        "0.1.1",
        &src,
        "127.0.0.1:50052".parse().unwrap(),
        &data,
        false,
    )
    .unwrap();
    let reg = load_service_registry().unwrap();
    assert_eq!(reg.instances.len(), 1);
    assert_eq!(reg.instances[0].listen.port(), 50052);
}

/// 真实 OS 服务安装（需管理员 / 会改系统）。默认忽略，手动：
/// `cargo test --test service_lifecycle os_service_install_uninstall -- --ignored --nocapture`
#[test]
#[ignore = "needs elevated privileges; mutates OS services"]
fn os_service_install_uninstall() {
    let _reg = RegistryGuard::new();
    let tmp = TempDir::new().unwrap();
    let install = tmp.path().join("app");
    let data = tmp.path().join("data");
    fs::create_dir_all(&data).unwrap();

    // 用当前测试二进制作为「源」不合适（不是 astral-core）；复制运行中的 astral-core 更理想。
    // 这里用 env 指定的程序，或跳过。
    let program = std::env::var_os("ASTRAL_CORE_TEST_PROGRAM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            // 尝试 debug 构建产物
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("debug")
                .join(binary_name())
        });
    if !program.exists() {
        eprintln!("skip: missing {program:?}; set ASTRAL_CORE_TEST_PROGRAM");
        return;
    }

    service::install(service::InstallOptions {
        listen: "127.0.0.1:50111".parse().unwrap(),
        data_dir: Some(data),
        program: Some(program),
        install_root: Some(install),
        version: Some("0.0.0-test".into()),
        retain: 2,
        user: cfg!(not(windows)),
        start_after_install: false,
    })
    .expect("install");

    let st = service::status(service::ServiceActionOptions {
        user: cfg!(not(windows)),
    })
    .expect("status");
    eprintln!("status after install: {st:?}");

    service::uninstall(service::UninstallOptions {
        user: cfg!(not(windows)),
        purge_data: false,
    })
    .expect("uninstall");
}
