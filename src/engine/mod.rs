//! EasyTier 引擎适配层。

mod events;
mod manager;
mod peers;
mod rpc;
mod structured;

pub use events::EventHub;
pub use manager::EngineHandle;
pub use peers::{hostname_from_info, my_ipv4_from_info, peer_summaries_from_info};
pub use rpc::ctrl;
pub use structured::{astral_to_et_network_config, loader_to_toml, structured_to_loader};
