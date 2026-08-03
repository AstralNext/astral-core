//! EasyTier 实例级 RPC 访问辅助。

use std::sync::Arc;

use easytier::instance_manager::NetworkInstanceManager;
use easytier::proto::rpc_types::controller::BaseController;
use easytier::rpc_service::InstanceRpcService;
use uuid::Uuid;

use crate::error::{CoreError, CoreResult};

/// 默认控制器。
pub fn ctrl() -> BaseController {
    BaseController::default()
}

/// 取运行中实例的 RPC 门面；未就绪则失败。
pub fn instance_rpc(
    manager: &NetworkInstanceManager,
    id: Uuid,
) -> CoreResult<Arc<dyn InstanceRpcService>> {
    manager
        .get_instance_service(&id)
        .ok_or_else(|| CoreError::FailedPrecondition(format!("实例 RPC 尚未就绪: {id}")))
}

/// 等待实例 RPC 可用（启动竞态）。
pub async fn wait_instance_rpc(
    manager: &Arc<NetworkInstanceManager>,
    id: Uuid,
    timeout: std::time::Duration,
) -> CoreResult<Arc<dyn InstanceRpcService>> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(svc) = manager.get_instance_service(&id) {
            return Ok(svc);
        }
        if std::time::Instant::now() >= deadline {
            return Err(CoreError::FailedPrecondition(format!(
                "等待实例 RPC 超时: {id}"
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
