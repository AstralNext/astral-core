//! 节点出站 Agent：dial 控制端、双向会话、RPC 隧道本地执行。

mod client;
mod credential;
mod dispatch;

pub use client::{run_agent_loop, AgentConnectConfig};
pub use credential::{load_device_credential, save_device_credential};
pub use dispatch::dispatch_tunnel_request;
