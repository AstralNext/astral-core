//! Windows SCM 服务主机（需与 `sc create` 注册的服务名一致）。

use std::ffi::OsString;
use std::sync::mpsc;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use tracing::{error, info, warn};
use windows_service::define_windows_service;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::service_dispatcher;

use super::run::{bootstrap_runtime, RunParams};
use crate::rpc;

static SERVICE_NAME: OnceLock<String> = OnceLock::new();
static RUN_PARAMS: OnceLock<RunParams> = OnceLock::new();

define_windows_service!(ffi_service_main, service_main);

/// 以 Windows 服务方式运行（由 SCM 拉起；阻塞至服务停止）。
pub fn run_as_windows_service(service_name: String, params: RunParams) -> Result<()> {
    SERVICE_NAME
        .set(service_name.clone())
        .map_err(|_| anyhow!("Windows 服务名已初始化"))?;
    RUN_PARAMS
        .set(params)
        .map_err(|_| anyhow!("运行参数已初始化"))?;

    service_dispatcher::start(service_name, ffi_service_main)
        .context("启动 Windows 服务调度失败（请确认由 SCM 拉起，且服务名匹配）")?;
    Ok(())
}

fn service_main(_arguments: Vec<OsString>) {
    if let Err(e) = run_service() {
        error!(error = %e, "Windows 服务运行失败");
    }
}

fn run_service() -> Result<()> {
    let service_name = SERVICE_NAME
        .get()
        .ok_or_else(|| anyhow!("缺少服务名"))?
        .clone();
    let params = RUN_PARAMS
        .get()
        .ok_or_else(|| anyhow!("缺少运行参数"))?
        .clone();

    let (shutdown_tx, shutdown_rx) = mpsc::channel::<()>();

    let event_handler = move |control| -> ServiceControlHandlerResult {
        match control {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = shutdown_tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let status_handle = service_control_handler::register(&service_name, event_handler)
        .context("注册服务控制处理器失败")?;

    status_handle
        .set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::StartPending,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 1,
            wait_hint: Duration::from_secs(30),
            process_id: None,
        })
        .context("设置 StartPending 状态失败")?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("创建 tokio runtime 失败")?;

    let bootstrapped = rt.block_on(async {
        let state = bootstrap_runtime(&params)?;
        Ok::<_, anyhow::Error>(state)
    });

    let state = match bootstrapped {
        Ok(s) => s,
        Err(e) => {
            warn!(error = %e, "服务引导失败");
            let _ = status_handle.set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Stopped,
                controls_accepted: ServiceControlAccept::empty(),
                exit_code: ServiceExitCode::Win32(1),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            });
            return Err(e);
        }
    };

    status_handle
        .set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })
        .context("设置 Running 状态失败")?;

    let result = rt.block_on(async move {
        let (async_stop_tx, async_stop_rx) = tokio::sync::oneshot::channel::<()>();

        std::thread::spawn(move || {
            let _ = shutdown_rx.recv();
            let _ = async_stop_tx.send(());
        });

        rpc::serve_with_shutdown(state, async move {
            let _ = async_stop_rx.await;
            info!("收到 Windows 服务停止请求");
        })
        .await
        .map_err(|e| anyhow!(e))
    });

    if let Err(e) = &result {
        warn!(error = %e, "服务主循环异常退出");
    }

    let _ = status_handle.set_service_status(ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: match &result {
            Ok(()) => ServiceExitCode::Win32(0),
            Err(_) => ServiceExitCode::Win32(1),
        },
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    });

    result
}
