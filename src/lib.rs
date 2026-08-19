//! astral-core 库入口。
//!
//! 本机单例内核：EasyTier 引擎 + 本机 JSON-RPC，只服务 Astral GUI。

#![deny(missing_docs)]

pub mod app;
pub mod config;
pub mod discovery;
pub mod engine;
pub mod error;
pub mod logging;
pub mod model;
pub mod rpc;
pub mod service;
pub mod store;

pub use app::AppState;
pub use error::{CoreError, CoreResult};
