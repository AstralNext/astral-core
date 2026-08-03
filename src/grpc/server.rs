//! 组装并启动 tonic gRPC 服务器（含 Bearer 鉴权拦截器）。

use std::net::SocketAddr;

use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::server::Router;
use tonic::transport::Server;
use tracing::info;

use crate::app::AppState;
use crate::auth::AuthInterceptor;
use crate::error::CoreResult;
use crate::pb::acl_service_server::AclServiceServer;
use crate::pb::app_message_service_server::AppMessageServiceServer;
use crate::pb::backup_service_server::BackupServiceServer;
use crate::pb::config_service_server::ConfigServiceServer;
use crate::pb::credential_service_server::CredentialServiceServer;
use crate::pb::event_service_server::EventServiceServer;
use crate::pb::instance_service_server::InstanceServiceServer;
use crate::pb::logger_service_server::LoggerServiceServer;
use crate::pb::network_service_server::NetworkServiceServer;
use crate::pb::node_service_server::NodeServiceServer;
use crate::pb::port_forward_service_server::PortForwardServiceServer;
use crate::pb::stats_service_server::StatsServiceServer;
use crate::pb::system_service_server::SystemServiceServer;
use crate::pb::vpn_portal_service_server::VpnPortalServiceServer;
use crate::services::{
    AclSvc, AppMessageSvc, BackupSvc, ConfigSvc, CredentialSvc, EventSvc, InstanceSvc, LoggerSvc,
    NetworkSvc, NodeSvc, PortForwardSvc, StatsSvc, SystemSvc, VpnSvc,
};

/// 在配置的地址上启动 gRPC，直到进程被取消。
pub async fn serve(state: AppState) -> CoreResult<()> {
    let addr = state.runtime.grpc_listen;
    info!(%addr, "启动 astral-core gRPC");
    build_router(state)
        .serve(addr)
        .await
        .map_err(|e| crate::error::CoreError::Internal(format!("gRPC 服务失败: {e}")))?;
    Ok(())
}

/// 在配置的地址上启动 gRPC，并在 `shutdown` 完成时退出。
pub async fn serve_with_shutdown<F>(state: AppState, shutdown: F) -> CoreResult<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let addr = state.runtime.grpc_listen;
    info!(%addr, "启动 astral-core gRPC（可关闭）");
    build_router(state)
        .serve_with_shutdown(addr, shutdown)
        .await
        .map_err(|e| crate::error::CoreError::Internal(format!("gRPC 服务失败: {e}")))?;
    Ok(())
}

/// 绑定已有 [`TcpListener`]（可为 `127.0.0.1:0`），并在 `shutdown` 完成时退出。
///
/// 供集成测试使用：先 `local_addr()` 再连接客户端。
pub async fn serve_with_incoming_shutdown<F>(
    state: AppState,
    listener: TcpListener,
    shutdown: F,
) -> CoreResult<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let addr = listener
        .local_addr()
        .map_err(|e| crate::error::CoreError::Internal(e.to_string()))?;
    info!(%addr, "启动 astral-core gRPC（可关闭）");
    build_router(state)
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown)
        .await
        .map_err(|e| crate::error::CoreError::Internal(format!("gRPC 服务失败: {e}")))?;
    Ok(())
}

/// 解析监听地址（测试辅助）。
pub fn listen_addr(state: &AppState) -> SocketAddr {
    state.runtime.grpc_listen
}

/// 组装完整 gRPC Router（含鉴权拦截器）。
pub fn build_router(state: AppState) -> Router {
    let auth = AuthInterceptor::new(state.tokens.clone());
    Server::builder()
        .add_service(SystemServiceServer::with_interceptor(
            SystemSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(InstanceServiceServer::with_interceptor(
            InstanceSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(NetworkServiceServer::with_interceptor(
            NetworkSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(CredentialServiceServer::with_interceptor(
            CredentialSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(NodeServiceServer::with_interceptor(
            NodeSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(EventServiceServer::with_interceptor(
            EventSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(ConfigServiceServer::with_interceptor(
            ConfigSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(LoggerServiceServer::with_interceptor(
            LoggerSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(VpnPortalServiceServer::with_interceptor(
            VpnSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(PortForwardServiceServer::with_interceptor(
            PortForwardSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(AclServiceServer::with_interceptor(
            AclSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(StatsServiceServer::with_interceptor(
            StatsSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(AppMessageServiceServer::with_interceptor(
            AppMessageSvc::new(state.clone()),
            auth.clone(),
        ))
        .add_service(BackupServiceServer::with_interceptor(
            BackupSvc::new(state),
            auth,
        ))
}
