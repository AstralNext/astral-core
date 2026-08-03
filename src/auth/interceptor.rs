//! gRPC 鉴权拦截器：校验 `authorization: Bearer <token>`。

use std::sync::Arc;

use tonic::metadata::MetadataMap;
use tonic::{Request, Status};

use super::TokenStore;

/// 附加到所有服务的鉴权拦截器。
#[derive(Clone)]
pub struct AuthInterceptor {
    store: Arc<TokenStore>,
}

impl AuthInterceptor {
    /// 创建拦截器。
    pub fn new(store: Arc<TokenStore>) -> Self {
        Self { store }
    }
}

impl tonic::service::Interceptor for AuthInterceptor {
    fn call(&mut self, request: Request<()>) -> Result<Request<()>, Status> {
        let token = extract_bearer(request.metadata()).ok_or_else(|| {
            Status::unauthenticated("缺少 authorization: Bearer <token>")
        })?;
        let ok = self
            .store
            .verify_plaintext(&token)
            .map_err(|e| Status::internal(e.to_string()))?;
        if !ok {
            return Err(Status::unauthenticated("无效或已吊销的 API Token"));
        }
        Ok(request)
    }
}

/// 从 metadata 提取 Bearer token。
fn extract_bearer(meta: &MetadataMap) -> Option<String> {
    let value = meta.get("authorization")?.to_str().ok()?;
    let rest = value.strip_prefix("Bearer ").or_else(|| value.strip_prefix("bearer "))?;
    let t = rest.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}
