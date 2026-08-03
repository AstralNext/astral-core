//! gRPC 场景化集成测试入口。
//!
//! ## 场景一览
//! | 模块 | 覆盖 |
//! |------|------|
//! | `scenario_auth` | 无/错/对 Token |
//! | `scenario_system` | Ping / Info / Capabilities |
//! | `scenario_credential` | Create/List/Revoke；禁吊销最后一把；未实现 RPC |
//! | `scenario_instance` | Validate→Start→Get/List/Meta/Config→Restart→Autostart→Stop/Delete |
//! | `scenario_node` | Self/List/Get/HostInfo；中控向未实现 |
//! | `scenario_network` | Start 后 Status/Peers/Collect/Routes/Local |
//! | `scenario_event` | Subscribe 快照流 |
//! | `scenario_config` | Get/Replace 缓存配置 |
//! | `scenario_stubs` | Logger/Backup/部分 Credential 返回 UNIMPLEMENTED |
//!
//! 约束：`no_tun=true`，适合 GitHub Actions。

mod harness;
mod scenario_auth;
mod scenario_config;
mod scenario_credential;
mod scenario_event;
mod scenario_instance;
mod scenario_network;
mod scenario_node;
mod scenario_stubs;
mod scenario_system;
