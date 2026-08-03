//! TLS 辅助：控制端服务端身份、Agent 出站客户端配置。

use std::fs;
use std::path::{Path, PathBuf};

use tonic::transport::{Certificate, ClientTlsConfig, Identity, ServerTlsConfig};

use crate::error::{CoreError, CoreResult};

/// 控制端 TLS 证书对（PEM）。
#[derive(Debug, Clone)]
pub struct ServerTlsPaths {
    /// 证书链 PEM。
    pub cert: PathBuf,
    /// 私钥 PEM。
    pub key: PathBuf,
}

impl ServerTlsPaths {
    /// 两者必须同时存在。
    pub fn from_opts(cert: Option<PathBuf>, key: Option<PathBuf>) -> CoreResult<Option<Self>> {
        match (cert, key) {
            (None, None) => Ok(None),
            (Some(cert), Some(key)) => Ok(Some(Self { cert, key })),
            _ => Err(CoreError::InvalidArgument(
                "--tls-cert 与 --tls-key 必须同时提供".into(),
            )),
        }
    }

    /// 加载为 tonic ServerTlsConfig。
    pub fn load_server_config(&self) -> CoreResult<ServerTlsConfig> {
        let cert = fs::read(&self.cert).map_err(|e| {
            CoreError::Internal(format!("读取 TLS 证书失败 {}: {e}", self.cert.display()))
        })?;
        let key = fs::read(&self.key).map_err(|e| {
            CoreError::Internal(format!("读取 TLS 私钥失败 {}: {e}", self.key.display()))
        })?;
        Ok(ServerTlsConfig::new().identity(Identity::from_pem(cert, key)))
    }
}

/// Agent 连接控制端时的 TLS 选项。
#[derive(Debug, Clone, Default)]
pub struct ClientTlsOpts {
    /// 自定义 CA / 自签服务器证书 PEM（自签时把服务端 cert 传这里）。
    pub ca_cert: Option<PathBuf>,
    /// SNI / 校验用域名；缺省取 URL host。
    pub domain: Option<String>,
}

/// 为 `https://` 端点构建 ClientTlsConfig。
pub fn build_client_tls(url: &str, opts: &ClientTlsOpts) -> CoreResult<ClientTlsConfig> {
    let uri: http::Uri = url
        .parse()
        .map_err(|e| CoreError::InvalidArgument(format!("无效 controller URL: {e}")))?;
    let mut tls = ClientTlsConfig::new();
    let domain = opts
        .domain
        .clone()
        .or_else(|| uri.host().map(|h| h.to_string()));
    if let Some(d) = domain {
        tls = tls.domain_name(d);
    }
    if let Some(ca) = &opts.ca_cert {
        let pem = fs::read(ca).map_err(|e| {
            CoreError::Internal(format!("读取 TLS CA 失败 {}: {e}", ca.display()))
        })?;
        tls = tls.ca_certificate(Certificate::from_pem(pem));
    } else {
        tls = tls.with_native_roots();
    }
    Ok(tls)
}

/// 路径存在性快速校验（安装服务前）。
pub fn require_readable(path: &Path, label: &str) -> CoreResult<()> {
    if !path.is_file() {
        return Err(CoreError::InvalidArgument(format!(
            "{label} 不是可读文件: {}",
            path.display()
        )));
    }
    let _ = fs::metadata(path).map_err(|e| {
        CoreError::InvalidArgument(format!("{label} 无法读取 {}: {e}", path.display()))
    })?;
    Ok(())
}
