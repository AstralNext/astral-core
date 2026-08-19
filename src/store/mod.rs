//! 运行中实例配置缓存（内存 + 落盘，供开机自动重连）。

mod cache;

pub use cache::{CachedInstance, InstanceCache};
