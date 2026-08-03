//! gRPC Service 实现集合。

mod acl;
mod app_message;
mod config_svc;
mod credential;
mod event;
mod instance;
mod network;
mod node;
mod portforward;
mod stats;
mod stubs;
mod system;
mod util;
mod vpn;

pub use acl::AclSvc;
pub use app_message::AppMessageSvc;
pub use config_svc::ConfigSvc;
pub use credential::CredentialSvc;
pub use event::EventSvc;
pub use instance::InstanceSvc;
pub use network::NetworkSvc;
pub use node::NodeSvc;
pub use portforward::PortForwardSvc;
pub use stats::StatsSvc;
pub use stubs::{BackupSvc, LoggerSvc};
pub use system::SystemSvc;
pub use vpn::VpnSvc;
