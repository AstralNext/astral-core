//! PortForwardService：列表 / 增删 / TCP 代理观测。

use std::net::SocketAddr as StdSocketAddr;

use easytier::proto::api::config::{
    ConfigPatchAction, InstanceConfigPatch, PatchConfigRequest, PortForwardPatch,
};
use easytier::proto::api::instance::{ListPortForwardRequest, ListTcpProxyEntryRequest};
use easytier::proto::common::{socket_addr, PortForwardConfigPb, SocketAddr, SocketType};
use easytier::proto::rpc_types::controller::BaseController;
use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::port_forward_service_server::PortForwardService;
use crate::pb::{
    AddPortForwardRequest, AddPortForwardResponse, ListPortForwardsRequest,
    ListPortForwardsResponse, ListTcpProxyEntriesRequest, ListTcpProxyEntriesResponse,
    PortForwardEntry, RemovePortForwardRequest, RemovePortForwardResponse, TcpProxyEntry,
};
use crate::services::util::{parse_instance_id, require_instance_id};

/// PortForwardService 服务端。
pub struct PortForwardSvc {
    state: AppState,
}

impl PortForwardSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn list_entries(&self, id: uuid::Uuid) -> Result<Vec<PortForwardEntry>, Status> {
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let resp = svc
            .get_port_forward_manage_service()
            .list_port_forward(
                BaseController::default(),
                ListPortForwardRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(resp.cfgs.into_iter().map(pb_to_entry).collect())
    }
}

fn pb_to_entry(c: PortForwardConfigPb) -> PortForwardEntry {
    let (bind_ip, bind_port) = split_sock(c.bind_addr.as_ref());
    let (dst_ip, dst_port) = split_sock(c.dst_addr.as_ref());
    PortForwardEntry {
        protocol: match c.socket_type() {
            SocketType::Udp => "udp".into(),
            _ => "tcp".into(),
        },
        bind_ip,
        bind_port,
        dst_ip,
        dst_port,
    }
}

fn split_sock(addr: Option<&SocketAddr>) -> (String, u32) {
    let Some(a) = addr else {
        return (String::new(), 0);
    };
    let ip = match &a.ip {
        Some(socket_addr::Ip::Ipv4(v)) => {
            let b = v.addr.to_be_bytes();
            format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
        }
        Some(socket_addr::Ip::Ipv6(v)) => format!("{v}"),
        None => String::new(),
    };
    (ip, a.port)
}

fn entry_to_pb(e: &PortForwardEntry) -> Result<PortForwardConfigPb, Status> {
    let bind: StdSocketAddr = format!("{}:{}", e.bind_ip, e.bind_port)
        .parse()
        .map_err(|err| Status::invalid_argument(format!("bind 无效: {err}")))?;
    let dst: StdSocketAddr = format!("{}:{}", e.dst_ip, e.dst_port)
        .parse()
        .map_err(|err| Status::invalid_argument(format!("dst 无效: {err}")))?;
    let socket_type = if e.protocol.eq_ignore_ascii_case("udp") {
        SocketType::Udp as i32
    } else {
        SocketType::Tcp as i32
    };
    Ok(PortForwardConfigPb {
        bind_addr: Some(bind.into()),
        dst_addr: Some(dst.into()),
        socket_type,
    })
}

#[tonic::async_trait]
impl PortForwardService for PortForwardSvc {
    async fn list_port_forwards(
        &self,
        req: Request<ListPortForwardsRequest>,
    ) -> Result<Response<ListPortForwardsResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        Ok(Response::new(ListPortForwardsResponse {
            entries: self.list_entries(id).await?,
        }))
    }

    async fn add_port_forward(
        &self,
        req: Request<AddPortForwardRequest>,
    ) -> Result<Response<AddPortForwardResponse>, Status> {
        let r = req.into_inner();
        let id = parse_instance_id(&require_instance_id(&r.instance)?)?;
        let entry = r
            .entry
            .ok_or_else(|| Status::invalid_argument("缺少 entry"))?;
        let cfg = entry_to_pb(&entry)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        svc.get_config_service()
            .patch_config(
                BaseController::default(),
                PatchConfigRequest {
                    patch: Some(InstanceConfigPatch {
                        port_forwards: vec![PortForwardPatch {
                            action: ConfigPatchAction::Add as i32,
                            cfg: Some(cfg),
                        }],
                        ..Default::default()
                    }),
                    instance: None,
                },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(AddPortForwardResponse {
            entries: self.list_entries(id).await?,
        }))
    }

    async fn remove_port_forward(
        &self,
        req: Request<RemovePortForwardRequest>,
    ) -> Result<Response<RemovePortForwardResponse>, Status> {
        let r = req.into_inner();
        let id = parse_instance_id(&require_instance_id(&r.instance)?)?;
        let entry = r
            .entry
            .ok_or_else(|| Status::invalid_argument("缺少 entry"))?;
        let cfg = entry_to_pb(&entry)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        svc.get_config_service()
            .patch_config(
                BaseController::default(),
                PatchConfigRequest {
                    patch: Some(InstanceConfigPatch {
                        port_forwards: vec![PortForwardPatch {
                            action: ConfigPatchAction::Remove as i32,
                            cfg: Some(cfg),
                        }],
                        ..Default::default()
                    }),
                    instance: None,
                },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        Ok(Response::new(RemovePortForwardResponse {
            entries: self.list_entries(id).await?,
        }))
    }

    async fn list_tcp_proxy_entries(
        &self,
        req: Request<ListTcpProxyEntriesRequest>,
    ) -> Result<Response<ListTcpProxyEntriesResponse>, Status> {
        let id = parse_instance_id(&require_instance_id(&req.into_inner().instance)?)?;
        let svc = self.state.engine.wait_rpc(id).await.map_err(Status::from)?;
        let Some(proxy) = svc.get_proxy_service("tcp") else {
            return Ok(Response::new(ListTcpProxyEntriesResponse { entries: vec![] }));
        };
        let resp = proxy
            .list_tcp_proxy_entry(
                BaseController::default(),
                ListTcpProxyEntryRequest { instance: None },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;
        let entries = resp
            .entries
            .into_iter()
            .map(|e| TcpProxyEntry {
                src: e.src.as_ref().map(|s| format!("{s:?}")).unwrap_or_default(),
                dst: e.dst.as_ref().map(|s| format!("{s:?}")).unwrap_or_default(),
                state: format!("{:?}", e.state()),
                rx_bytes: 0,
                tx_bytes: 0,
                raw_json: serde_json::to_string(&e).unwrap_or_default(),
            })
            .collect();
        Ok(Response::new(ListTcpProxyEntriesResponse { entries }))
    }
}
