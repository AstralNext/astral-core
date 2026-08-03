//! gRPC 服务器装配。

mod server;

pub use server::{
    build_router, listen_addr, serve, serve_with_incoming_shutdown, serve_with_shutdown,
};
