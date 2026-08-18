//! EasyTier 引擎适配层。

mod manager;
mod peers;
mod structured;

pub use manager::EngineHandle;
pub use peers::{
    has_local_peer, hostname_from_info, merge_local_peer, my_ipv4_from_info, peer_summaries_from_info,
};
pub use structured::{astral_to_et_network_config, loader_to_toml, structured_to_loader};
