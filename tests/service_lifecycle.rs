//! 服务布局 / 登记 / 更新测试（不依赖系统服务权限）。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use astral_core::service::{
    binary_name, load_service_registry, program_path, record_install, record_program,
    record_uninstall, service_label, stage_binary, update, UpdateOptions,
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
fn service_label_is_qualified_name() {
    let label = service_label().unwrap();
    assert_eq!(label.to_qualified_name(), "dev.astral.core");
    assert_eq!(label.to_script_name(), "astral-core");
}

#[test]
fn layout_fixed_program_path() {
    let root = TempDir::new().unwrap();
    let root = root.path();
    let src1 = root.join("download").join(binary_name());
    let src2 = root.join("download2").join(binary_name());
    write_fake_bin(&src1, "v1");
    write_fake_bin(&src2, "v2");

    let install = root.join("app");
    fs::create_dir_all(&install).unwrap();

    let staged = stage_binary(&install, &src1).unwrap();
    assert_eq!(staged, program_path(&install));
    assert_eq!(read_marker(&program_path(&install)), "fake-bin:v1");

    // 同路径覆盖：更新 = 覆盖 exe
    stage_binary(&install, &src2).unwrap();
    assert_eq!(read_marker(&program_path(&install)), "fake-bin:v2");
}

#[cfg(windows)]
#[test]
fn stage_binary_copies_wintun_sidecar() {
    let root = TempDir::new().unwrap();
    let src_dir = root.path().join("download");
    fs::create_dir_all(&src_dir).unwrap();
    let src = src_dir.join(binary_name());
    write_fake_bin(&src, "core");
    fs::write(src_dir.join("wintun.dll"), b"signed-wintun").unwrap();
    fs::write(src_dir.join("Packet.dll"), b"packet").unwrap();

    let install = root.path().join("app");
    fs::create_dir_all(&install).unwrap();
    stage_binary(&install, &src).unwrap();

    assert_eq!(
        fs::read(install.join("wintun.dll")).unwrap(),
        b"signed-wintun"
    );
    assert_eq!(fs::read(install.join("Packet.dll")).unwrap(), b"packet");
}

#[test]
fn update_overwrites_fixed_path() {
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

    // 无登记也允许 bootstrap（带 install-root）
    update(UpdateOptions {
        program: Some(s1),
        install_root: Some(install.clone()),
        no_start: true,
    })
    .expect("bootstrap update");

    let reg = load_service_registry().unwrap();
    assert!(reg.install_root.is_some());
    assert_eq!(read_marker(&program_path(&install)), "fake-bin:a");

    update(UpdateOptions {
        program: Some(s2),
        install_root: Some(install.clone()),
        no_start: true,
    })
    .unwrap();
    assert_eq!(read_marker(&program_path(&install)), "fake-bin:b");
    assert_eq!(
        load_service_registry().unwrap().program,
        Some(program_path(&install))
    );
}

#[test]
fn registry_record_install_and_uninstall() {
    let _reg = RegistryGuard::new();
    let tmp = TempDir::new().unwrap();
    let install = tmp.path().join("app");
    let data = tmp.path().join("data");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(&data).unwrap();
    let prog = program_path(&install);
    write_fake_bin(&prog, "x");

    record_install(
        &install,
        &prog,
        "127.0.0.1:50051".parse().unwrap(),
        &data,
        false,
    )
    .unwrap();

    let reg = load_service_registry().unwrap();
    assert_eq!(reg.instances.len(), 1);
    assert_eq!(reg.instances[0].name, "core");
    assert_eq!(reg.program, Some(prog));

    record_uninstall().unwrap();
    let reg = load_service_registry().unwrap();
    assert!(reg.instances.is_empty());
    assert!(reg.install_root.is_none());
}

#[test]
fn registry_record_program_updates_path() {
    let _reg = RegistryGuard::new();
    let tmp = TempDir::new().unwrap();
    let install = tmp.path().join("app");
    let data = tmp.path().join("data");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(&data).unwrap();
    let prog = program_path(&install);
    write_fake_bin(&prog, "x");

    record_install(
        &install,
        &prog,
        "127.0.0.1:50051".parse().unwrap(),
        &data,
        false,
    )
    .unwrap();

    let install2 = tmp.path().join("app2");
    fs::create_dir_all(&install2).unwrap();
    let prog2 = program_path(&install2);
    write_fake_bin(&prog2, "y");
    record_program(&install2, &prog2).unwrap();

    let reg = load_service_registry().unwrap();
    assert_eq!(reg.install_root, Some(install2));
    assert_eq!(reg.program, Some(prog2));
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

    let program = std::env::var_os("ASTRAL_CORE_TEST_PROGRAM")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target")
                .join("debug")
                .join(binary_name())
        });
    if !program.exists() {
        eprintln!("skip: missing {program:?}; set ASTRAL_CORE_TEST_PROGRAM");
        return;
    }

    use astral_core::service;
    service::install(service::InstallOptions {
        listen: "127.0.0.1:50111".parse().unwrap(),
        data_dir: Some(data),
        program: Some(program),
        install_root: Some(install),
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
