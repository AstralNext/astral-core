//! 核心错误类型。

use thiserror::Error;

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

    /// 前置条件失败（如配置非法）。
    #[error("前置条件失败: {0}")]
    FailedPrecondition(String),

    /// JSON-RPC 方法不存在。
    #[error("未知方法: {0}")]
    MethodNotFound(String),

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
    /// JSON-RPC 2.0 错误码。
    pub fn rpc_code(&self) -> i64 {
        match self {
            CoreError::InvalidArgument(_) => -32602,
            CoreError::NotFound(_) => -32004,
            CoreError::FailedPrecondition(_) => -32002,
            CoreError::MethodNotFound(_) => -32601,
            CoreError::Internal(_) | CoreError::Io(_) | CoreError::Json(_) | CoreError::Other(_) => {
                -32603
            }
        }
    }
}
