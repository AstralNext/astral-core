//! NetworkService：运行中实例的网络视图。

use std::time::{SystemTime, UNIX_EPOCH};

use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::network_service_server::NetworkService;
use crate::pb::{
    CollectNetworkInfoRequest, CollectNetworkInfoResponse, ConnectorEntry, DumpRoutesRequest,
    DumpRoutesResponse, ForeignNetworkEntry, GetForeignNetworkSummaryRequest,
    GetForeignNetworkSummaryResponse, GetNetworkStatusRequest, GetNetworkStatusResponse,
    GetPeerRequest, GetPeerResponse, ListConnectorsRequest, ListConnectorsResponse,
    ListForeignNetworksRequest, ListForeignNetworksResponse, ListMappedListenersRequest,
    ListMappedListenersResponse, ListPeersRequest, ListPeersResponse, ListPublicIpv6InfoRequest,
    ListPublicIpv6InfoResponse, ListRoutesRequest, ListRoutesResponse, MappedListenerEntry,
    PublicIpv6Entry, RouteEntry, ShowLocalNodeInfoRequest, ShowLocalNodeInfoResponse,
};
use crate::services::util::{parse_instance_id, require_instance_id, ts_from_unix};

/// NetworkService 服务端。
pub struct NetworkSvc {
    state: AppState,
}

impl NetworkSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl NetworkService for NetworkSvc {
    async fn get_network_status(
        &self,
        req: Request<GetNetworkStatusRequest>,
    ) -> Result<Response<GetNetworkStatusResponse>, Status> {
        let id_str = require_instance_id(&req.into_inner().instance)?;
        let id = parse_instance_id(&id_str)?;
        // 在表中但 info 未就绪：返回 running=false，不报 NOT_FOUND
        if !self.state.engine.exists(id) {
            return Err(Status::not_found(format!("实例不存在: {id_str}")));
        }
        let summary = self.state.engine.summary_of(id).await;
        let peers = self.state.engine.list_peers(id).await;
        let (my_ipv4, my_ipv6, hostname) = self.state.engine.local_addrs(id).await;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        Ok(Response::new(GetNetworkStatusResponse {
            instance_id: id_str,
            running: summary.running,
            state: summary.state,
            error_message: summary.error_message,
            dev_name: summary.dev_name,
            my_ipv4,
            my_ipv6,
            hostname,
            network_name: summary.network_name,
            peer_count: peers.len() as u32,
            peers,
            collected_at: Some(ts_from_unix(now)),
        }))
    }

    async fn list_peers(
        &self,
        req: Request<ListPeersRequest>,
    ) -> Result<Response<ListPeersResponse>, Status> {
        let r = req.into_inner();
        let id_str = require_instance_id(&r.instance)?;
        let id = parse_instance_id(&id_str)?;
        if !self.state.engine.exists(id) {
            return Err(Status::not_found(format!("实例不存在: {id_str}")));
        }
        let _ = r.include_dead;
        let peers = self.state.engine.list_peers(id).await;
        Ok(Response::new(ListPeersResponse { peers }))
    }

    async fn get_peer(
        &self,
        req: Request<GetPeerRequest>,
    ) -> Result<Response<GetPeerResponse>, Status> {
        let r = req.into_inner();
        let id_str = require_instance_id(&r.instance)?;
        let id = parse_instance_id(&id_str)?;
        let peers = self.state.engine.list_peers(id).await;
        let summary = peers
            .into_iter()
            .find(|p| p.peer_id == r.peer_id)
            .ok_or_else(|| Status::not_found(format!("peer 不存在: {}", r.peer_id)))?;
        Ok(Response::new(GetPeerResponse {
            peer: Some(crate::pb::PeerDetail {
                summary: Some(summary),
                version: String::new(),
                connections: vec![],
                raw_json: String::new(),
            }),
        }))
    }

    async fn collect_network_info(
        &self,
        req: Request<CollectNetworkInfoRequest>,
    ) -> Result<Response<CollectNetworkInfoResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        Ok(Response::new(CollectNetworkInfoResponse {
            raw_json: self.state.engine.running_info_json(id).await,
        }))
    }

    async fn list_routes(
        &self,
        req: Request<ListRoutesRequest>,
    ) -> Result<Response<ListRoutesResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_peer_manage_service()
            .list_route(
                easytier::proto::rpc_types::controller::BaseController::default(),
                easytier::proto::api::instance::ListRouteRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let routes = resp
            .routes
            .into_iter()
            .map(|r| RouteEntry {
                destination: r
                    .ipv4_addr
                    .as_ref()
                    .map(|a| a.to_string())
                    .unwrap_or_default(),
                next_hop: r.next_hop_peer_id.to_string(),
                cost: r.cost as u32,
                proxy_cidr: !r.proxy_cidrs.is_empty(),
                path: vec![],
            })
            .collect();
        Ok(Response::new(ListRoutesResponse { routes }))
    }

    async fn dump_routes(
        &self,
        req: Request<DumpRoutesRequest>,
    ) -> Result<Response<DumpRoutesResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_peer_manage_service()
            .dump_route(
                easytier::proto::rpc_types::controller::BaseController::default(),
                easytier::proto::api::instance::DumpRouteRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(DumpRoutesResponse {
            text: resp.result,
            raw_json: String::new(),
        }))
    }

    async fn show_local_node_info(
        &self,
        req: Request<ShowLocalNodeInfoRequest>,
    ) -> Result<Response<ShowLocalNodeInfoResponse>, Status> {
        let id_str = require_instance_id(&req.into_inner().instance)?;
        let id = parse_instance_id(&id_str)?;
        let (ipv4, ipv6, hostname) = self.state.engine.local_addrs(id).await;
        let peer_id = self
            .state
            .engine
            .running_info(id)
            .await
            .and_then(|i| i.my_node_info.map(|n| n.peer_id.to_string()))
            .unwrap_or_default();
        Ok(Response::new(ShowLocalNodeInfoResponse {
            peer_id,
            hostname,
            ipv4,
            ipv6,
            listeners: vec![],
            raw_json: self.state.engine.running_info_json(id).await,
        }))
    }

    async fn list_foreign_networks(
        &self,
        req: Request<ListForeignNetworksRequest>,
    ) -> Result<Response<ListForeignNetworksResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_peer_manage_service()
            .list_foreign_network(
                easytier::proto::rpc_types::controller::BaseController::default(),
                easytier::proto::api::instance::ListForeignNetworkRequest {
                    instance: None,
                    include_trusted_keys: false,
                },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let networks = resp
            .foreign_networks
            .into_iter()
            .map(|(name, entry)| ForeignNetworkEntry {
                network_name: name,
                raw_json: serde_json::to_string(&entry).unwrap_or_default(),
            })
            .collect();
        Ok(Response::new(ListForeignNetworksResponse { networks }))
    }

    async fn get_foreign_network_summary(
        &self,
        req: Request<GetForeignNetworkSummaryRequest>,
    ) -> Result<Response<GetForeignNetworkSummaryResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_peer_manage_service()
            .get_foreign_network_summary(
                easytier::proto::rpc_types::controller::BaseController::default(),
                easytier::proto::api::instance::GetForeignNetworkSummaryRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(GetForeignNetworkSummaryResponse {
            raw_json: serde_json::to_string(&resp).unwrap_or_default(),
        }))
    }

    async fn list_connectors(
        &self,
        req: Request<ListConnectorsRequest>,
    ) -> Result<Response<ListConnectorsResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_connector_manage_service()
            .list_connector(
                easytier::proto::rpc_types::controller::BaseController::default(),
                easytier::proto::api::instance::ListConnectorRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let connectors = resp
            .connectors
            .into_iter()
            .map(|c| ConnectorEntry {
                url: format!("{:?}", c.url),
                status: format!("{:?}", c),
                raw_json: serde_json::to_string(&c).unwrap_or_default(),
            })
            .collect();
        Ok(Response::new(ListConnectorsResponse { connectors }))
    }

    async fn list_mapped_listeners(
        &self,
        req: Request<ListMappedListenersRequest>,
    ) -> Result<Response<ListMappedListenersResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_mapped_listener_manage_service()
            .list_mapped_listener(
                easytier::proto::rpc_types::controller::BaseController::default(),
                easytier::proto::api::instance::ListMappedListenerRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let listeners = resp
            .mappedlisteners
            .into_iter()
            .map(|l| MappedListenerEntry {
                url: format!("{l:?}"),
                raw_json: serde_json::to_string(&l).unwrap_or_default(),
            })
            .collect();
        Ok(Response::new(ListMappedListenersResponse { listeners }))
    }

    async fn list_public_ipv6_info(
        &self,
        req: Request<ListPublicIpv6InfoRequest>,
    ) -> Result<Response<ListPublicIpv6InfoResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_peer_manage_service()
            .list_public_ipv6_info(
                easytier::proto::rpc_types::controller::BaseController::default(),
                easytier::proto::api::instance::ListPublicIpv6InfoRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let mut entries = Vec::new();
        if let Some(prefix) = resp.provider_prefix {
            entries.push(PublicIpv6Entry {
                addr: prefix.to_string(),
                raw_json: serde_json::to_string(&prefix).unwrap_or_default(),
            });
        }
        for lease in resp.provider_leases {
            entries.push(PublicIpv6Entry {
                addr: format!("{lease:?}"),
                raw_json: serde_json::to_string(&lease).unwrap_or_default(),
            });
        }
        Ok(Response::new(ListPublicIpv6InfoResponse { entries }))
    }
}
