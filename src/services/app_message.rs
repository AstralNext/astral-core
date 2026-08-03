//! AppMessageService：astral_app_rpc Call / Notify / Reply / 入站流。

use std::pin::Pin;
use std::time::Duration;

use easytier::peers::astral_app_rpc::{self, AppInboundEvent as EtInbound};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::app_message_service_server::AppMessageService;
use crate::pb::{
    AppCallRequest, AppCallResponse, AppInboundEvent, AppInboundKind, AppNotifyRequest,
    AppNotifyResponse, AppReplyRequest, AppReplyResponse, SubscribeAppInboundRequest,
};
use crate::services::util::{parse_instance_id, require_instance_id, ts_from_unix};

/// AppMessageService 服务端。
pub struct AppMessageSvc {
    /// 保留以便后续扩展（如按节点限流）。
    #[allow(dead_code)]
    state: AppState,
}

impl AppMessageSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

async fn wait_app(id: uuid::Uuid) -> Result<std::sync::Arc<astral_app_rpc::AstralAppRpcService>, Status> {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(s) = astral_app_rpc::get_service(&id) {
            return Ok(s);
        }
        if std::time::Instant::now() >= deadline {
            return Err(Status::failed_precondition(format!(
                "astral_app_rpc 未就绪: {id}"
            )));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

type InboundStream = Pin<Box<dyn Stream<Item = Result<AppInboundEvent, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl AppMessageService for AppMessageSvc {
    type SubscribeInboundStream = InboundStream;

    async fn call(
        &self,
        req: Request<AppCallRequest>,
    ) -> Result<Response<AppCallResponse>, Status> {
        let r = req.into_inner();
        let id = parse_instance_id(&require_instance_id(&r.instance)?)?;
        let peer: u32 = r
            .to_peer_id
            .parse()
            .map_err(|_| Status::invalid_argument("to_peer_id 须为 u32"))?;
        let timeout = if r.timeout_ms == 0 {
            5000
        } else {
            r.timeout_ms as i32
        };
        let svc = wait_app(id).await?;
        let resp = svc
            .call(peer, r.method, 0, r.payload, 0, timeout)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(AppCallResponse {
            status: resp.status,
            payload: resp.payload,
            error_message: resp.error_msg,
        }))
    }

    async fn notify(
        &self,
        req: Request<AppNotifyRequest>,
    ) -> Result<Response<AppNotifyResponse>, Status> {
        let r = req.into_inner();
        let id = parse_instance_id(&require_instance_id(&r.instance)?)?;
        let peer: u32 = r
            .to_peer_id
            .parse()
            .map_err(|_| Status::invalid_argument("to_peer_id 须为 u32"))?;
        let svc = wait_app(id).await?;
        svc.notify(peer, r.method, r.payload, 5000)
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(AppNotifyResponse {}))
    }

    async fn reply(
        &self,
        req: Request<AppReplyRequest>,
    ) -> Result<Response<AppReplyResponse>, Status> {
        let r = req.into_inner();
        let id = parse_instance_id(&require_instance_id(&r.instance)?)?;
        let svc = wait_app(id).await?;
        let ok = svc.reply_call(r.token, r.status, r.error_message, r.payload);
        Ok(Response::new(AppReplyResponse { ok }))
    }

    async fn subscribe_inbound(
        &self,
        req: Request<SubscribeAppInboundRequest>,
    ) -> Result<Response<Self::SubscribeInboundStream>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = wait_app(id).await?;
        let mut et_rx = svc.subscribe_inbound();
        drop(svc);
        let (tx, rx) = mpsc::channel(64);
        tokio::spawn(async move {
            loop {
                match et_rx.recv().await {
                    Ok(evt) => {
                        let mapped = match evt {
                            EtInbound::Call {
                                from_peer_id,
                                channel,
                                request_id,
                                token,
                                payload,
                            } => AppInboundEvent {
                                kind: AppInboundKind::Call as i32,
                                from_peer_id: from_peer_id.to_string(),
                                method: channel,
                                payload,
                                token,
                                request_id,
                                timestamp: Some(ts_from_unix(now_secs())),
                            },
                            EtInbound::Notify {
                                from_peer_id,
                                channel,
                                payload,
                            } => AppInboundEvent {
                                kind: AppInboundKind::Notify as i32,
                                from_peer_id: from_peer_id.to_string(),
                                method: channel,
                                payload,
                                token: 0,
                                request_id: 0,
                                timestamp: Some(ts_from_unix(now_secs())),
                            },
                        };
                        if tx.send(Ok(mapped)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
