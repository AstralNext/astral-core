//! 构建脚本：从旁路仓库 `astral-api` 编译 astral.v1 protobuf / gRPC 代码。
//!
//! 约定：`astral-api` 与 `astral-core` 为同级目录：
//! ```text
//! GitHub/
//!   astral-api/proto/...
//!   astral-core/   ← 本包
//! ```

use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    // CI 可设 ASTRAL_API_PROTO；本地默认 ../astral-api/proto
    let proto_root = env::var_os("ASTRAL_API_PROTO")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("..").join("astral-api").join("proto"));

    if !proto_root.is_dir() {
        panic!(
            "找不到 astral-api proto 目录: {}（请保证 astral-api 与 astral-core 同级，或设置 ASTRAL_API_PROTO）",
            proto_root.display()
        );
    }

    let files = [
        "astral/v1/common.proto",
        "astral/v1/system.proto",
        "astral/v1/instance.proto",
        "astral/v1/network.proto",
        "astral/v1/config.proto",
        "astral/v1/event.proto",
        "astral/v1/credential.proto",
        "astral/v1/node.proto",
        "astral/v1/logger.proto",
        "astral/v1/vpn.proto",
        "astral/v1/portforward.proto",
        "astral/v1/acl.proto",
        "astral/v1/stats.proto",
        "astral/v1/app_message.proto",
        "astral/v1/backup.proto",
    ];

    let abs: Vec<PathBuf> = files.iter().map(|f| proto_root.join(f)).collect();
    for f in &abs {
        println!("cargo:rerun-if-changed={}", f.display());
    }
    println!("cargo:rerun-if-changed={}", proto_root.display());

    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(&abs, &[proto_root])?;

    Ok(())
}
