//! 将带 `x-astral-node-id` 的 gRPC 调用经隧道转到对应节点。

use std::collections::HashMap;
use std::task::{Context, Poll};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use http::{Request, Response};
use http_body_util::{BodyExt, Full};
use tonic::body::BoxBody;
use tonic::Status;
use tower::Service;
use tracing::debug;

use crate::controller::sessions::SessionRegistry;
use crate::controller::META_NODE_ID;

/// 包装内层 Router：有节点元数据则隧道代理，否则交给本地中控服务。
#[derive(Clone)]
pub struct AgentProxyService<S> {
    inner: S,
    sessions: SessionRegistry,
}

impl<S> AgentProxyService<S> {
    /// 创建。
    pub fn new(inner: S, sessions: SessionRegistry) -> Self {
        Self { inner, sessions }
    }
}

impl<S> Service<Request<BoxBody>> for AgentProxyService<S>
where
    S: Service<Request<BoxBody>, Response = Response<BoxBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Into<Box<dyn std::error::Error + Send + Sync>> + Send,
{
    type Response = Response<BoxBody>;
    type Error = S::Error;
    type Future = futures_util::future::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<BoxBody>) -> Self::Future {
        let node_id = req
            .headers()
            .get(META_NODE_ID)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if let Some(node_id) = node_id {
            let sessions = self.sessions.clone();
            let mut inner = self.inner.clone();
            // 需要 ready 内层吗？代理路径不调用内层
            let _ = &mut inner;
            Box::pin(async move {
                match proxy_request(sessions, node_id, req).await {
                    Ok(resp) => Ok(resp),
                    Err(status) => Ok(status_to_http(status)),
                }
            })
        } else {
            let mut inner = self.inner.clone();
            Box::pin(async move { inner.call(req).await })
        }
    }
}

async fn proxy_request(
    sessions: SessionRegistry,
    node_id: String,
    req: Request<BoxBody>,
) -> Result<Response<BoxBody>, Status> {
    let full_method = req.uri().path().to_string();
    // AgentSession 本身不能再套隧道
    if full_method.ends_with("AgentSession") {
        return Err(Status::invalid_argument(
            "AgentSession 不能经 x-astral-node-id 代理",
        ));
    }

    let mut metadata = HashMap::new();
    for (k, v) in req.headers().iter() {
        let key = k.as_str();
        if key == META_NODE_ID || key.starts_with(':') {
            continue;
        }
        if let Ok(val) = v.to_str() {
            metadata.insert(key.to_string(), val.to_string());
        }
    }

    let body = req
        .into_body()
        .collect()
        .await
        .map_err(|e| Status::internal(format!("读请求体失败: {e}")))?
        .to_bytes();
    let payload = decode_grpc_payload(&body)?;

    debug!(%node_id, %full_method, "经隧道代理到节点");
    let tun = sessions
        .proxy_unary(&node_id, &full_method, payload, metadata)
        .await
        .map_err(|e| Status::unavailable(e.to_string()))?;

    if tun.grpc_status != 0 {
        return Err(Status::new(
            code_from_i32(tun.grpc_status),
            tun.message,
        ));
    }

    let body = encode_grpc_payload(&tun.payload);
    Response::builder()
        .status(200)
        .header("content-type", "application/grpc")
        .header("grpc-status", "0")
        .body(BoxBody::new(Full::new(body).map_err(|e| match e {})))
        .map_err(|e| Status::internal(e.to_string()))
}

fn decode_grpc_payload(body: &Bytes) -> Result<Vec<u8>, Status> {
    if body.len() < 5 {
        return Err(Status::invalid_argument("gRPC 帧过短"));
    }
    let compressed = body[0];
    if compressed != 0 {
        return Err(Status::unimplemented("不支持压缩 gRPC 帧"));
    }
    let len = (&body[1..5]).get_u32() as usize;
    if body.len() < 5 + len {
        return Err(Status::invalid_argument("gRPC 帧长度不匹配"));
    }
    Ok(body[5..5 + len].to_vec())
}

fn encode_grpc_payload(msg: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(5 + msg.len());
    buf.put_u8(0);
    buf.put_u32(msg.len() as u32);
    buf.extend_from_slice(msg);
    buf.freeze()
}

fn code_from_i32(code: i32) -> tonic::Code {
    tonic::Code::from_i32(code)
}

fn status_to_http(status: Status) -> Response<BoxBody> {
    status_to_http_response(status)
}

/// 将 tonic Status 转为 gRPC-over-HTTP 响应（供 AdminGate 等复用）。
pub fn status_to_http_response(status: Status) -> Response<BoxBody> {
    let body = encode_grpc_payload(b"");
    Response::builder()
        .status(200)
        .header("content-type", "application/grpc")
        .header("grpc-status", (status.code() as i32).to_string())
        .header("grpc-message", percent_encode(status.message()))
        .body(BoxBody::new(Full::new(body).map_err(|e| match e {})))
        .unwrap_or_else(|_| {
            Response::new(BoxBody::new(Full::new(Bytes::new()).map_err(|e| match e {})))
        })
}

fn percent_encode(s: &str) -> String {
    // grpc-message 需要百分号编码；简单处理
    s.chars()
        .map(|c| match c {
            ' ' => "%20".to_string(),
            c if c.is_ascii_alphanumeric() || "-_.~".contains(c) => c.to_string(),
            c => format!("%{:02X}", c as u8),
        })
        .collect()
}
