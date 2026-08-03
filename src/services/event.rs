//! EventService：订阅 EasyTier EventBus 转发的事件流。

use std::pin::Pin;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::app::AppState;
use crate::pb::event::Payload;
use crate::pb::event_service_server::EventService;
use crate::pb::{Event, EventType, InstanceStateChanged, SubscribeEventsRequest};
use crate::services::util::{ensure_local_node, ts_from_unix};

/// EventService 服务端。
pub struct EventSvc {
    state: AppState,
}

impl EventSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

type EventStream = Pin<Box<dyn Stream<Item = Result<Event, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl EventService for EventSvc {
    type SubscribeEventsStream = EventStream;

    async fn subscribe_events(
        &self,
        req: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        let r = req.into_inner();
        ensure_local_node(&r.node, &self.state.node_id)?;

        let hub = self
            .state
            .engine
            .events()
            .cloned()
            .ok_or_else(|| Status::internal("EventHub 未初始化"))?;

        let types = r.types;
        let default_types = types.is_empty();
        let instance_filter = r.instance_ids;
        let include_snapshots = r.include_snapshots;
        let engine = self.state.engine.clone();
        let node_id = self.state.node_id.clone();

        let (tx, rx) = mpsc::channel::<Result<Event, Status>>(128);

        tokio::spawn(async move {
            let type_ok = |ty: i32| -> bool {
                if default_types {
                    ty == EventType::InstanceState as i32
                        || ty == EventType::PeerChanged as i32
                } else {
                    types.iter().any(|t| *t == ty)
                }
            };

            if include_snapshots && type_ok(EventType::InstanceState as i32) {
                for s in engine.list_summaries().await {
                    if !instance_filter.is_empty() && !instance_filter.contains(&s.instance_id) {
                        continue;
                    }
                    let ev = Event {
                        event_id: Uuid::new_v4().to_string(),
                        timestamp: Some(ts_from_unix(now_secs())),
                        node_id: node_id.clone(),
                        instance_id: s.instance_id.clone(),
                        r#type: EventType::InstanceState as i32,
                        payload: Some(Payload::InstanceState(InstanceStateChanged {
                            instance_id: s.instance_id,
                            state: s.state,
                            running: s.running,
                            error_message: s.error_message,
                            dev_name: s.dev_name,
                        })),
                    };
                    if tx.send(Ok(ev)).await.is_err() {
                        return;
                    }
                }
            }

            let mut sub = hub.subscribe();
            loop {
                match sub.recv().await {
                    Ok(mut ev) => {
                        if !instance_filter.is_empty()
                            && !ev.instance_id.is_empty()
                            && !instance_filter.contains(&ev.instance_id)
                        {
                            continue;
                        }
                        if !type_ok(ev.r#type) {
                            continue;
                        }
                        if ev.r#type == EventType::PeerChanged as i32 {
                            if let Some(Payload::PeerChanged(ref mut pc)) = ev.payload {
                                if pc.peers.is_empty() {
                                    if let Ok(id) = Uuid::parse_str(&ev.instance_id) {
                                        pc.peers = engine.list_peers(id).await;
                                    }
                                }
                            }
                        }
                        if tx.send(Ok(ev)).await.is_err() {
                            return;
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
