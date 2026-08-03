//! astral-core 库入口。
//!
//! 模块划分：
//! - [`error`]：统一错误类型
//! - [`config`]：数据目录、运行配置
//! - [`auth`]：API Token 存储与 gRPC 鉴权拦截器
//! - [`engine`]：EasyTier 实例管理适配
//! - [`pb`]：由 build.rs 生成的 astral.v1 绑定
//! - [`services`]：各 gRPC Service 实现
//! - [`grpc`]：服务器装配与启动
//! - [`app`]：进程级状态与引导
//! - [`store`]：实例配置缓存与自启登记
//! - [`service`]：跨平台系统服务安装 / 启停（service-manager）
//! - [`wizard`]：本地部署 TUI 向导
//! - [`tls_util`]：控制端 / Agent TLS 辅助
//! - [`agent`]：出站连接控制端（双向会话 / RPC 隧道）
//! - [`controller`]：控制端 listen（收 Agent、按节点代理 RPC）

#![deny(missing_docs)]

pub mod agent;
pub mod app;
pub mod auth;
pub mod config;
pub mod controller;
pub mod engine;
pub mod error;
pub mod grpc;
pub mod pb;
pub mod service;
pub mod services;
pub mod store;
pub mod tls_util;
pub mod wizard;

pub use app::AppState;
pub use error::{CoreError, CoreResult};
