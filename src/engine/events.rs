//! 基于 EasyTier EventBus 的全局事件中枢（供 EventService 订阅）。

use std::sync::Arc;

use easytier::common::global_ctx::GlobalCtxEvent;
use easytier::common::global_ctx::EventBusSubscriber;
use tokio::sync::broadcast;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::pb::{
    event::Payload, ControlErrorEvent, Event, EventType, InstanceState, InstanceStateChanged,
    PeerChanged,
};

fn ts_from_unix(secs: i64) -> crate::pb::Timestamp {
    crate::pb::Timestamp {
        seconds: secs,
        nanos: 0,
    }
}

/// 进程内事件广播。
#[derive(Clone)]
pub struct EventHub {
    tx: broadcast::Sender<Event>,
    node_id: Arc<String>,
}

impl EventHub {
    /// 创建；`capacity` 为广播缓冲。
    pub fn new(node_id: String, capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self {
            tx,
            node_id: Arc::new(node_id),
        }
    }

    /// 订阅。
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }

    /// 手动发布（状态轮询兜底 / 快照）。
    pub fn publish(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    /// 为本实例挂上 EasyTier EventBus 转发。
    pub fn attach_easytier_bus(&self, instance_id: Uuid, mut sub: EventBusSubscriber) {
        let hub = self.clone();
        let id_str = instance_id.to_string();
        tokio::spawn(async move {
            loop {
                match sub.recv().await {
                    Ok(ev) => {
                        if let Some(mapped) = map_et_event(&hub.node_id, &id_str, ev) {
                            let _ = hub.tx.send(mapped);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        debug!(instance_id = %id_str, "ET EventBus 已关闭");
                        break;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!(instance_id = %id_str, dropped = n, "ET EventBus 滞后丢弃");
                        let _ = hub.tx.send(make_control_error(
                            &hub.node_id,
                            &id_str,
                            "EVENT_LAGGED",
                            format!("丢弃 {n} 条引擎事件"),
                        ));
                    }
                }
            }
        });
    }

    /// 节点 ID。
    pub fn node_id(&self) -> &str {
        &self.node_id
    }
}

fn now_ts() -> crate::pb::Timestamp {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    ts_from_unix(secs)
}

fn base_event(node_id: &str, instance_id: &str, ty: EventType) -> Event {
    Event {
        event_id: Uuid::new_v4().to_string(),
        timestamp: Some(now_ts()),
        node_id: node_id.to_string(),
        instance_id: instance_id.to_string(),
        r#type: ty as i32,
        payload: None,
    }
}

fn make_control_error(node_id: &str, instance_id: &str, code: &str, message: String) -> Event {
    let mut e = base_event(node_id, instance_id, EventType::ControlError);
    e.payload = Some(Payload::ControlError(ControlErrorEvent {
        code: code.into(),
        message,
        instance_id: instance_id.into(),
    }));
    e
}

fn map_et_event(node_id: &str, instance_id: &str, ev: GlobalCtxEvent) -> Option<Event> {
    match ev {
        GlobalCtxEvent::PeerAdded(_)
        | GlobalCtxEvent::PeerRemoved(_)
        | GlobalCtxEvent::PeerConnAdded(_)
        | GlobalCtxEvent::PeerConnRemoved(_) => {
            // 细粒度 peer 变化：发空 peers 占位，由 EventSvc 可选再拉全量
            let mut e = base_event(node_id, instance_id, EventType::PeerChanged);
            e.payload = Some(Payload::PeerChanged(PeerChanged {
                instance_id: instance_id.into(),
                peers: vec![],
            }));
            Some(e)
        }
        GlobalCtxEvent::TunDeviceReady(dev) => {
            let mut e = base_event(node_id, instance_id, EventType::InstanceState);
            e.payload = Some(Payload::InstanceState(InstanceStateChanged {
                instance_id: instance_id.into(),
                state: InstanceState::Running as i32,
                running: true,
                error_message: String::new(),
                dev_name: dev,
            }));
            Some(e)
        }
        GlobalCtxEvent::TunDeviceError(err) => {
            let mut e = base_event(node_id, instance_id, EventType::InstanceState);
            e.payload = Some(Payload::InstanceState(InstanceStateChanged {
                instance_id: instance_id.into(),
                state: InstanceState::Error as i32,
                running: false,
                error_message: err,
                dev_name: String::new(),
            }));
            Some(e)
        }
        GlobalCtxEvent::ListenerAddFailed(_, msg)
        | GlobalCtxEvent::ListenerAcceptFailed(_, msg)
        | GlobalCtxEvent::ConnectError(_, _, msg)
        | GlobalCtxEvent::ConnectionError(_, _, msg) => {
            Some(make_control_error(node_id, instance_id, "ENGINE_ERROR", msg))
        }
        GlobalCtxEvent::ConfigPatched(_) => {
            let mut e = base_event(node_id, instance_id, EventType::ConfigChanged);
            e.payload = Some(Payload::ConfigChanged(crate::pb::ConfigChangedEvent {
                instance_id: instance_id.into(),
                revision: String::new(),
            }));
            Some(e)
        }
        GlobalCtxEvent::VpnPortalStarted(_)
        | GlobalCtxEvent::VpnPortalClientConnected(_, _)
        | GlobalCtxEvent::VpnPortalClientDisconnected(_, _)
        | GlobalCtxEvent::PortForwardAdded(_)
        | GlobalCtxEvent::PublicIpv6Changed(_, _)
        | GlobalCtxEvent::PublicIpv6RoutesUpdated(_, _)
        | GlobalCtxEvent::DhcpIpv4Changed(_, _)
        | GlobalCtxEvent::DhcpIpv4Conflicted(_)
        | GlobalCtxEvent::ListenerAdded(_)
        | GlobalCtxEvent::ListenerPortMappingEstablished { .. }
        | GlobalCtxEvent::Connecting(_)
        | GlobalCtxEvent::ConnectionAccepted(_, _)
        | GlobalCtxEvent::UdpBroadcastRelayStartResult { .. }
        | GlobalCtxEvent::CredentialChanged
        | GlobalCtxEvent::ProxyCidrsUpdated(_, _) => {
            // 归入控制错误通道的轻量通知，便于 Subscribe 默认也能看到
            Some(make_control_error(
                node_id,
                instance_id,
                "ENGINE_EVENT",
                format!("{ev:?}"),
            ))
        }
    }
}
