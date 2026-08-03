//! 进程启动时的运行参数。

use std::net::SocketAddr;
use std::path::PathBuf;

/// gRPC 与引擎相关的运行时配置。
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// gRPC 监听地址（默认 127.0.0.1:50051）。
    pub grpc_listen: SocketAddr,
    /// 可选：覆盖数据目录。
    pub data_dir: Option<PathBuf>,
    /// 对外公布的 core 版本字符串。
    pub core_version: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            grpc_listen: "127.0.0.1:50051".parse().expect("静态地址"),
            data_dir: None,
            core_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// [`RuntimeConfig`] 构建器。
#[derive(Debug, Default)]
pub struct RuntimeConfigBuilder {
    inner: RuntimeConfig,
}

impl RuntimeConfigBuilder {
    /// 新建构建器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置 gRPC 监听地址。
    pub fn grpc_listen(mut self, addr: SocketAddr) -> Self {
        self.inner.grpc_listen = addr;
        self
    }

    /// 设置数据目录覆盖。
    pub fn data_dir(mut self, dir: PathBuf) -> Self {
        self.inner.data_dir = Some(dir);
        self
    }

    /// 完成构建。
    pub fn build(self) -> RuntimeConfig {
        self.inner
    }
}
