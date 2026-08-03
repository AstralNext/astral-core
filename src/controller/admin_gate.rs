//! 控制端管理 API 鉴权：除 AgentSession 外均需 Bearer。

use std::task::{Context, Poll};

use http::Request;
use tonic::body::BoxBody;
use tonic::Status;
use tower::Service;

/// 包装内层服务：非 AgentSession 路径要求 `authorization: Bearer <admin_token>`。
#[derive(Clone)]
pub struct AdminGateService<S> {
    inner: S,
    admin_token: String,
}

impl<S> AdminGateService<S> {
    /// 创建。
    pub fn new(inner: S, admin_token: String) -> Self {
        Self { inner, admin_token }
    }
}

impl<S> Service<Request<BoxBody>> for AdminGateService<S>
where
    S: Service<Request<BoxBody>, Response = http::Response<BoxBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
{
    type Response = http::Response<BoxBody>;
    type Error = S::Error;
    type Future = futures_util::future::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<BoxBody>) -> Self::Future {
        let path = req.uri().path().to_string();
        let is_agent_session = path.ends_with("/AgentSession") || path.ends_with("AgentSession");
        if is_agent_session {
            let mut inner = self.inner.clone();
            return Box::pin(async move { inner.call(req).await });
        }

        let token_ok = extract_bearer(req.headers()).is_some_and(|t| t == self.admin_token);
        if !token_ok {
            let status = Status::unauthenticated(
                "控制端管理 API 需要 authorization: Bearer <token>（与 --token 相同）",
            );
            return Box::pin(async move { Ok(crate::controller::proxy::status_to_http_response(status)) });
        }

        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await })
    }
}

fn extract_bearer(headers: &http::HeaderMap) -> Option<String> {
    let value = headers.get("authorization")?.to_str().ok()?;
    let rest = value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))?;
    let t = rest.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}
