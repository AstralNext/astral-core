//! 进程启动时的运行参数。

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;

use crate::error::{CoreError, CoreResult};

/// JSON-RPC 与引擎相关的运行时配置。
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// JSON-RPC 监听地址（仅 loopback）。
    pub listen: SocketAddr,
    /// 可选：覆盖数据目录。
    pub data_dir: Option<PathBuf>,
    /// 对外公布的 core 版本字符串。
    pub core_version: String,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:50051".parse().expect("静态地址"),
            data_dir: None,
            core_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// 强制本机回环。`0.0.0.0` / `::` 改写为 localhost；其它公网/局域网地址拒绝。
pub fn require_local_listen(addr: SocketAddr) -> CoreResult<SocketAddr> {
    if addr.ip().is_loopback() {
        return Ok(addr);
    }
    if addr.ip().is_unspecified() {
        let ip = if addr.is_ipv4() {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            IpAddr::V6(Ipv6Addr::LOCALHOST)
        };
        return Ok(SocketAddr::new(ip, addr.port()));
    }
    Err(CoreError::InvalidArgument(format!(
        "astral-core 仅监听本机，不支持 {addr}"
    )))
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

    /// 设置 JSON-RPC 监听地址（非本机地址会在 [`Self::build`] 时拒绝）。
    pub fn listen(mut self, addr: SocketAddr) -> Self {
        self.inner.listen = addr;
        self
    }

    /// 设置数据目录覆盖。
    pub fn data_dir(mut self, dir: PathBuf) -> Self {
        self.inner.data_dir = Some(dir);
        self
    }

    /// 完成构建。
    pub fn build(self) -> CoreResult<RuntimeConfig> {
        let mut cfg = self.inner;
        cfg.listen = require_local_listen(cfg.listen)?;
        Ok(cfg)
    }
}
