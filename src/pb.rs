//! astral.v1 protobuf / gRPC 生成代码入口。
//!
//! 由 `build.rs` 调用 `tonic-build` 从 `astral-api/proto` 生成。
//! 字段号与合同以 astral-api 仓库为准，本模块禁止手改生成物。

#![allow(missing_docs)]

/// `package astral.v1` 下全部 message / service。
pub mod astral {
    pub mod v1 {
        tonic::include_proto!("astral.v1");
    }
}

pub use astral::v1::*;
