//! 运行配置与数据目录。

mod paths;
mod settings;

pub use paths::DataPaths;
pub use settings::{require_local_listen, RuntimeConfig, RuntimeConfigBuilder};
