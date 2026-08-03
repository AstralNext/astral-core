//! 将隧道 unary 请求分发到本机 Service 实现（与直连 RPC 同一套逻辑）。

use prost::Message;
use tonic::{Request, Status};
use tracing::debug;

use crate::app::AppState;
use crate::pb::acl_service_server::AclService;
use crate::pb::config_service_server::ConfigService;
use crate::pb::credential_service_server::CredentialService;
use crate::pb::instance_service_server::InstanceService;
use crate::pb::network_service_server::NetworkService;
use crate::pb::node_service_server::NodeService;
use crate::pb::port_forward_service_server::PortForwardService;
use crate::pb::stats_service_server::StatsService;
use crate::pb::system_service_server::SystemService;
use crate::pb::vpn_portal_service_server::VpnPortalService;
use crate::pb::*;
use crate::services::{
    AclSvc, ConfigSvc, CredentialSvc, InstanceSvc, NetworkSvc, NodeSvc, PortForwardSvc, StatsSvc,
    SystemSvc, VpnSvc,
};

/// 执行一次隧道代理调用，返回 (grpc_status, message, payload)。
pub async fn dispatch_tunnel_request(
    state: &AppState,
    full_method: &str,
    payload: &[u8],
) -> (i32, String, Vec<u8>) {
    match invoke(state, full_method, payload).await {
        Ok(bytes) => (0, String::new(), bytes),
        Err(status) => (status.code() as i32, status.message().to_string(), Vec::new()),
    }
}

async fn invoke(state: &AppState, full_method: &str, payload: &[u8]) -> Result<Vec<u8>, Status> {
    debug!(%full_method, "隧道本地执行");
    let method = full_method.trim();
    // 兼容带/不带前导包名
    let method = method.strip_prefix('/').unwrap_or(method);

    macro_rules! unary {
        ($Svc:ident, $call:ident, $Req:ty) => {{
            let req = <$Req>::decode(payload)
                .map_err(|e| Status::invalid_argument(format!("解码请求失败: {e}")))?;
            let resp = $Svc::new(state.clone())
                .$call(Request::new(req))
                .await?
                .into_inner();
            Ok(resp.encode_to_vec())
        }};
    }

    match method {
        // —— System ——
        "astral.v1.SystemService/Ping" => unary!(SystemSvc, ping, PingRequest),
        "astral.v1.SystemService/GetServerInfo" => {
            unary!(SystemSvc, get_server_info, GetServerInfoRequest)
        }
        "astral.v1.SystemService/GetCapabilities" => {
            unary!(SystemSvc, get_capabilities, GetCapabilitiesRequest)
        }

        // —— Node（不含 AgentSession / 控制面 enroll）——
        "astral.v1.NodeService/GetSelfNode" => unary!(NodeSvc, get_self_node, GetSelfNodeRequest),
        "astral.v1.NodeService/ListNodes" => unary!(NodeSvc, list_nodes, ListNodesRequest),
        "astral.v1.NodeService/GetNode" => unary!(NodeSvc, get_node, GetNodeRequest),
        "astral.v1.NodeService/RenameNode" => unary!(NodeSvc, rename_node, RenameNodeRequest),
        "astral.v1.NodeService/GetNodeHostInfo" => {
            unary!(NodeSvc, get_node_host_info, GetNodeHostInfoRequest)
        }

        // —— Instance ——
        "astral.v1.InstanceService/ValidateConfig" => {
            unary!(InstanceSvc, validate_config, ValidateConfigRequest)
        }
        "astral.v1.InstanceService/StartInstance" => {
            unary!(InstanceSvc, start_instance, StartInstanceRequest)
        }
        "astral.v1.InstanceService/StopInstance" => {
            unary!(InstanceSvc, stop_instance, StopInstanceRequest)
        }
        "astral.v1.InstanceService/RestartInstance" => {
            unary!(InstanceSvc, restart_instance, RestartInstanceRequest)
        }
        "astral.v1.InstanceService/ListInstances" => {
            unary!(InstanceSvc, list_instances, ListInstancesRequest)
        }
        "astral.v1.InstanceService/GetInstance" => {
            unary!(InstanceSvc, get_instance, GetInstanceRequest)
        }
        "astral.v1.InstanceService/DeleteInstance" => {
            unary!(InstanceSvc, delete_instance, DeleteInstanceRequest)
        }
        "astral.v1.InstanceService/RetainInstances" => {
            unary!(InstanceSvc, retain_instances, RetainInstancesRequest)
        }
        "astral.v1.InstanceService/GetInstanceConfig" => {
            unary!(InstanceSvc, get_instance_config, GetInstanceConfigRequest)
        }
        "astral.v1.InstanceService/ListInstanceMeta" => {
            unary!(InstanceSvc, list_instance_meta, ListInstanceMetaRequest)
        }
        "astral.v1.InstanceService/SetAutostart" => {
            unary!(InstanceSvc, set_autostart, SetAutostartRequest)
        }
        "astral.v1.InstanceService/ListAutostart" => {
            unary!(InstanceSvc, list_autostart, ListAutostartRequest)
        }
        "astral.v1.InstanceService/ListProfiles" => {
            unary!(InstanceSvc, list_profiles, ListProfilesRequest)
        }
        "astral.v1.InstanceService/GetProfile" => unary!(InstanceSvc, get_profile, GetProfileRequest),
        "astral.v1.InstanceService/UpsertProfile" => {
            unary!(InstanceSvc, upsert_profile, UpsertProfileRequest)
        }
        "astral.v1.InstanceService/DeleteProfile" => {
            unary!(InstanceSvc, delete_profile, DeleteProfileRequest)
        }
        "astral.v1.InstanceService/StartProfile" => {
            unary!(InstanceSvc, start_profile, StartProfileRequest)
        }

        // —— Network ——
        "astral.v1.NetworkService/GetNetworkStatus" => {
            unary!(NetworkSvc, get_network_status, GetNetworkStatusRequest)
        }
        "astral.v1.NetworkService/ListPeers" => unary!(NetworkSvc, list_peers, ListPeersRequest),
        "astral.v1.NetworkService/GetPeer" => unary!(NetworkSvc, get_peer, GetPeerRequest),
        "astral.v1.NetworkService/CollectNetworkInfo" => {
            unary!(NetworkSvc, collect_network_info, CollectNetworkInfoRequest)
        }
        "astral.v1.NetworkService/ListRoutes" => unary!(NetworkSvc, list_routes, ListRoutesRequest),
        "astral.v1.NetworkService/DumpRoutes" => unary!(NetworkSvc, dump_routes, DumpRoutesRequest),
        "astral.v1.NetworkService/ShowLocalNodeInfo" => {
            unary!(NetworkSvc, show_local_node_info, ShowLocalNodeInfoRequest)
        }
        "astral.v1.NetworkService/ListForeignNetworks" => {
            unary!(NetworkSvc, list_foreign_networks, ListForeignNetworksRequest)
        }
        "astral.v1.NetworkService/GetForeignNetworkSummary" => {
            unary!(
                NetworkSvc,
                get_foreign_network_summary,
                GetForeignNetworkSummaryRequest
            )
        }
        "astral.v1.NetworkService/ListConnectors" => {
            unary!(NetworkSvc, list_connectors, ListConnectorsRequest)
        }
        "astral.v1.NetworkService/ListMappedListeners" => {
            unary!(NetworkSvc, list_mapped_listeners, ListMappedListenersRequest)
        }
        "astral.v1.NetworkService/ListPublicIpv6Info" => {
            unary!(NetworkSvc, list_public_ipv6_info, ListPublicIpv6InfoRequest)
        }

        // —— Config ——
        "astral.v1.ConfigService/GetConfig" => unary!(ConfigSvc, get_config, GetConfigRequest),
        "astral.v1.ConfigService/PatchConfig" => unary!(ConfigSvc, patch_config, PatchConfigRequest),
        "astral.v1.ConfigService/ReplaceConfig" => {
            unary!(ConfigSvc, replace_config, ReplaceConfigRequest)
        }

        // —— Credential ——
        "astral.v1.CredentialService/CreateToken" => {
            unary!(CredentialSvc, create_token, CreateTokenRequest)
        }
        "astral.v1.CredentialService/ListTokens" => {
            unary!(CredentialSvc, list_tokens, ListTokensRequest)
        }
        "astral.v1.CredentialService/RevokeToken" => {
            unary!(CredentialSvc, revoke_token, RevokeTokenRequest)
        }
        "astral.v1.CredentialService/RotateToken" => {
            unary!(CredentialSvc, rotate_token, RotateTokenRequest)
        }

        // —— Stats ——
        "astral.v1.StatsService/GetStats" => unary!(StatsSvc, get_stats, GetStatsRequest),
        "astral.v1.StatsService/GetPrometheusMetrics" => {
            unary!(StatsSvc, get_prometheus_metrics, GetPrometheusMetricsRequest)
        }

        // —— ACL ——
        "astral.v1.AclService/GetWhitelist" => unary!(AclSvc, get_whitelist, GetWhitelistRequest),
        "astral.v1.AclService/PatchWhitelist" => {
            unary!(AclSvc, patch_whitelist, PatchWhitelistRequest)
        }
        "astral.v1.AclService/GetAclStats" => unary!(AclSvc, get_acl_stats, GetAclStatsRequest),
        "astral.v1.AclService/GetAcl" => unary!(AclSvc, get_acl, GetAclRequest),
        "astral.v1.AclService/SetAcl" => unary!(AclSvc, set_acl, SetAclRequest),

        // —— PortForward ——
        "astral.v1.PortForwardService/ListPortForwards" => {
            unary!(PortForwardSvc, list_port_forwards, ListPortForwardsRequest)
        }
        "astral.v1.PortForwardService/AddPortForward" => {
            unary!(PortForwardSvc, add_port_forward, AddPortForwardRequest)
        }
        "astral.v1.PortForwardService/RemovePortForward" => {
            unary!(PortForwardSvc, remove_port_forward, RemovePortForwardRequest)
        }
        "astral.v1.PortForwardService/ListTcpProxyEntries" => {
            unary!(PortForwardSvc, list_tcp_proxy_entries, ListTcpProxyEntriesRequest)
        }

        // —— VPN ——
        "astral.v1.VpnPortalService/GetVpnPortalInfo" => {
            unary!(VpnSvc, get_vpn_portal_info, GetVpnPortalInfoRequest)
        }
        "astral.v1.VpnPortalService/EnableVpnPortal" => {
            unary!(VpnSvc, enable_vpn_portal, EnableVpnPortalRequest)
        }
        "astral.v1.VpnPortalService/DisableVpnPortal" => {
            unary!(VpnSvc, disable_vpn_portal, DisableVpnPortalRequest)
        }

        _ => Err(Status::unimplemented(format!(
            "隧道尚未代理该方法: /{method}"
        ))),
    }
}
