//! 实例配置缓存与配置库 / 自启登记（Core 侧职责，非 EasyTier）。

mod autostart;
mod cache;
mod profile;

pub use autostart::AutostartStore;
pub use cache::{CachedInstance, InstanceCache};
pub use profile::{ProfileRecord, ProfileStore};
