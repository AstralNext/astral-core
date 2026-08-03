//! 尚未完全实现的 Service 占位（Backup / Logger）。

use std::pin::Pin;

use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::app::AppState;
use crate::pb::backup_service_server::BackupService;
use crate::pb::logger_service_server::LoggerService;
use crate::pb::*;

fn unimp(name: &str) -> Status {
    Status::unimplemented(format!("{name} 尚未实现"))
}

/// Logger 占位。
pub struct LoggerSvc {
    #[allow(dead_code)]
    state: AppState,
}

impl LoggerSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

/// Backup 占位。
pub struct BackupSvc {
    #[allow(dead_code)]
    state: AppState,
}

impl BackupSvc {
    /// 创建。
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl LoggerService for LoggerSvc {
    type SubscribeLogsStream =
        Pin<Box<dyn Stream<Item = Result<LogLine, Status>> + Send + 'static>>;

    async fn get_logger_config(
        &self,
        _: Request<GetLoggerConfigRequest>,
    ) -> Result<Response<GetLoggerConfigResponse>, Status> {
        Err(unimp("GetLoggerConfig"))
    }
    async fn set_logger_config(
        &self,
        _: Request<SetLoggerConfigRequest>,
    ) -> Result<Response<SetLoggerConfigResponse>, Status> {
        Err(unimp("SetLoggerConfig"))
    }
    async fn subscribe_logs(
        &self,
        _: Request<SubscribeLogsRequest>,
    ) -> Result<Response<Self::SubscribeLogsStream>, Status> {
        Err(unimp("SubscribeLogs"))
    }
    async fn get_recent_logs(
        &self,
        _: Request<GetRecentLogsRequest>,
    ) -> Result<Response<GetRecentLogsResponse>, Status> {
        Err(unimp("GetRecentLogs"))
    }
}

#[tonic::async_trait]
impl BackupService for BackupSvc {
    async fn export_bundle(
        &self,
        _: Request<ExportBundleRequest>,
    ) -> Result<Response<ExportBundleResponse>, Status> {
        Err(unimp("ExportBundle"))
    }
    async fn import_bundle(
        &self,
        _: Request<ImportBundleRequest>,
    ) -> Result<Response<ImportBundleResponse>, Status> {
        Err(unimp("ImportBundle"))
    }
    async fn list_remote_backups(
        &self,
        _: Request<ListRemoteBackupsRequest>,
    ) -> Result<Response<ListRemoteBackupsResponse>, Status> {
        Err(unimp("ListRemoteBackups"))
    }
    async fn push_backup(
        &self,
        _: Request<PushBackupRequest>,
    ) -> Result<Response<PushBackupResponse>, Status> {
        Err(unimp("PushBackup"))
    }
    async fn pull_backup(
        &self,
        _: Request<PullBackupRequest>,
    ) -> Result<Response<PullBackupResponse>, Status> {
        Err(unimp("PullBackup"))
    }
}
