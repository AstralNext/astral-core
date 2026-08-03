//! 核心错误类型与到 `tonic::Status` 的映射。

use thiserror::Error;
use tonic::Status;

/// 库内通用 Result。
pub type CoreResult<T> = Result<T, CoreError>;

/// astral-core 业务与基础设施错误。
#[derive(Debug, Error)]
pub enum CoreError {
    /// 参数不合法。
    #[error("无效参数: {0}")]
    InvalidArgument(String),

    /// 资源不存在。
    #[error("未找到: {0}")]
    NotFound(String),

    /// 前置条件失败（如配置非法、禁止吊销最后一把 token）。
    #[error("前置条件失败: {0}")]
    FailedPrecondition(String),

    /// 未实现的能力。
    #[error("尚未实现: {0}")]
    Unimplemented(String),

    /// 内部错误。
    #[error("内部错误: {0}")]
    Internal(String),

    /// IO 错误。
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// JSON 错误。
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// 其它 anyhow。
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl CoreError {
    /// 转为 gRPC Status（供 Service 实现使用）。
    pub fn into_status(self) -> Status {
        match self {
            CoreError::InvalidArgument(m) => Status::invalid_argument(m),
            CoreError::NotFound(m) => Status::not_found(m),
            CoreError::FailedPrecondition(m) => Status::failed_precondition(m),
            CoreError::Unimplemented(m) => Status::unimplemented(m),
            CoreError::Internal(m) => Status::internal(m),
            CoreError::Io(e) => Status::internal(e.to_string()),
            CoreError::Json(e) => Status::internal(e.to_string()),
            CoreError::Other(e) => Status::internal(e.to_string()),
        }
    }
}

impl From<CoreError> for Status {
    fn from(value: CoreError) -> Self {
        value.into_status()
    }
}
