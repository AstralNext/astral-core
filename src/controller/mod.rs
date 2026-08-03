//! 控制端：listen 收节点出站会话，按 node_id 隧道代理 astral.v1。

mod admin_gate;
mod auth;
mod hub;
mod hub_node;
mod proxy;
mod sessions;

pub use auth::ControllerAuth;
pub use hub::{run_controller, ControllerListenParams};
pub use sessions::SessionRegistry;

/// 客户端指定目标节点的 metadata 键。
pub const META_NODE_ID: &str = "x-astral-node-id";
