//! EasyTier 引擎适配层。

mod manager;
mod peers;
mod structured;

pub use manager::{EngineHandle, RestoreReport};
pub use peers::{
    has_local_peer, hostname_from_info, merge_local_peer, my_ipv4_from_info,
    peer_summaries_from_info,
};
pub use structured::{astral_to_et_network_config, loader_to_toml, structured_to_loader};

/// 开机后按落盘记录拉起实例；失败则指数退避重试。
pub async fn restore_desired_with_retry(engine: EngineHandle) {
    use std::time::Duration;
    use tracing::warn;

    let mut delay = Duration::from_secs(2);
    for attempt in 1..=6u32 {
        let report = engine.restore_desired();
        if report.failed.is_empty() {
            return;
        }
        if attempt == 6 {
            warn!(
                attempt,
                failed = report.failed.len(),
                "组网实例自动恢复仍未完成"
            );
            return;
        }
        warn!(
            attempt,
            failed = report.failed.len(),
            "组网实例自动恢复未完成，将重试"
        );
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(30));
    }
}
