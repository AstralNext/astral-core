//! StatsService：流量统计与 Prometheus。

use std::pin::Pin;
use std::time::Duration;

use easytier::proto::api::instance::{GetPrometheusStatsRequest, GetStatsRequest as EtGetStats};
use easytier::proto::rpc_types::controller::BaseController;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::stats_service_server::StatsService;
use crate::pb::{
    GetPrometheusMetricsRequest, GetPrometheusMetricsResponse, GetStatsRequest, GetStatsResponse,
    StatsSnapshot, SubscribeStatsRequest,
};
use crate::services::util::{parse_instance_id, require_instance_id, ts_from_unix};

/// StatsService 服务端。
pub struct StatsSvc {
    state: AppState,
}

impl StatsSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn collect(&self, id: uuid::Uuid) -> Result<(u64, u64, u32, String), Status> {
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_stats_service()
            .get_stats(BaseController::default(), EtGetStats { instance: None })
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let raw = serde_json::to_string(&resp.metrics).unwrap_or_default();
        let mut rx = 0u64;
        let mut tx = 0u64;
        for m in &resp.metrics {
            let name = m.name.to_lowercase();
            if name.contains("rx") || name.contains("recv") {
                rx = rx.saturating_add(m.value as u64);
            }
            if name.contains("tx") || name.contains("send") {
                tx = tx.saturating_add(m.value as u64);
            }
        }
        let peer_count = self.state.engine.list_peers(id).await.len() as u32;
        Ok((rx, tx, peer_count, raw))
    }
}

type StatsStream = Pin<Box<dyn Stream<Item = Result<StatsSnapshot, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl StatsService for StatsSvc {
    type SubscribeStatsStream = StatsStream;

    async fn get_stats(
        &self,
        req: Request<GetStatsRequest>,
    ) -> Result<Response<GetStatsResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let (rx, tx, peer_count, raw) = self.collect(id).await?;
        Ok(Response::new(GetStatsResponse {
            rx_bytes: rx,
            tx_bytes: tx,
            peer_count,
            raw_json: raw,
            collected_at: Some(ts_from_unix(now_secs())),
        }))
    }

    async fn get_prometheus_metrics(
        &self,
        req: Request<GetPrometheusMetricsRequest>,
    ) -> Result<Response<GetPrometheusMetricsResponse>, Status> {
        let r = req.into_inner();
        let id = if let Some(inst) = r.instance {
            parse_instance_id(&inst.instance_id)?
        } else {
            return Err(Status::invalid_argument("需要 instance"));
        };
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_stats_service()
            .get_prometheus_stats(
                BaseController::default(),
                GetPrometheusStatsRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetPrometheusMetricsResponse {
            text: resp.prometheus_text,
        }))
    }

    async fn subscribe_stats(
        &self,
        req: Request<SubscribeStatsRequest>,
    ) -> Result<Response<Self::SubscribeStatsStream>, Status> {
        let r = req.into_inner();
        let id_str = require_instance_id(&r.instance)?;
        let id = parse_instance_id(&id_str)?;
        let interval = Duration::from_millis(r.interval_ms.max(200) as u64);
        let engine = self.state.engine.clone();
        let (tx, rx) = mpsc::channel(16);
        tokio::spawn(async move {
            loop {
                let snap = match collect_static(&engine, id).await {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        break;
                    }
                };
                if tx.send(Ok(snap)).await.is_err() {
                    break;
                }
                tokio::time::sleep(interval).await;
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}

async fn collect_static(
    engine: &crate::engine::EngineHandle,
    id: uuid::Uuid,
) -> Result<StatsSnapshot, Status> {
    let svc = engine.wait_rpc(id).await.map_err(Status::from)?;
    let resp = svc
        .get_stats_service()
        .get_stats(BaseController::default(), EtGetStats { instance: None })
        .await
        .map_err(|e| Status::internal(e.to_string()))?;
    let raw = serde_json::to_string(&resp.metrics).unwrap_or_default();
    Ok(StatsSnapshot {
        instance_id: id.to_string(),
        rx_bytes: 0,
        tx_bytes: 0,
        peer_count: engine.list_peers(id).await.len() as u32,
        timestamp: Some(ts_from_unix(now_secs())),
        raw_json: raw,
    })
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
