//! 出站连接循环：dial → AgentSession → 握手/心跳/隧道 → 断线退避重连。

use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::Channel;
use tracing::{info, warn};

use crate::agent::credential::{load_device_credential, save_device_credential};
use crate::agent::dispatch::dispatch_tunnel_request;
use crate::app::AppState;
use crate::error::{CoreError, CoreResult};
use crate::pb::node_service_client::NodeServiceClient;
use crate::pb::{
    agent_session_frame, AgentHandshakeRequest, AgentHeartbeatRequest, AgentSessionFrame,
    AgentTunnelResponse,
};
use crate::tls_util::{build_client_tls, ClientTlsOpts};

/// 出站 Agent 连接配置。
#[derive(Debug, Clone)]
pub struct AgentConnectConfig {
    /// 控制端地址，如 `http://127.0.0.1:8443` 或 `https://ctrl.example:8443`。
    pub controller: String,
    /// 共享密钥：首次作 enroll_token，并用于校验 controller_attestation。
    pub token: String,
    /// 重连初始等待。
    pub retry_base: Duration,
    /// 重连最大等待。
    pub retry_max: Duration,
    /// 心跳间隔。
    pub heartbeat_interval: Duration,
    /// `https://` 时的 TLS 选项（自签请设 ca_cert）。
    pub tls: ClientTlsOpts,
}

/// 会话结果：区分「握手前失败」与「曾成功在线后断开」。
enum SessionOutcome {
    /// 握手成功后会话结束（应复位退避）。
    EstablishedThenEnded(CoreResult<()>),
    /// 握手前失败（加倍退避）。
    HandshakeFailed(CoreError),
}

/// 在后台持续维持与控制端的会话（直到进程取消）。
pub async fn run_agent_loop(state: AppState, cfg: AgentConnectConfig) {
    let mut backoff = cfg.retry_base;
    loop {
        match run_session(&state, &cfg).await {
            SessionOutcome::EstablishedThenEnded(Ok(())) => {
                info!("Agent 会话正常结束，准备重连");
                backoff = cfg.retry_base;
            }
            SessionOutcome::EstablishedThenEnded(Err(e)) => {
                warn!(error = %e, "Agent 会话断开，准备重连");
                backoff = cfg.retry_base;
            }
            SessionOutcome::HandshakeFailed(e) => {
                warn!(error = %e, retry_in = ?backoff, "Agent 握手/连接失败");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(cfg.retry_max);
                continue;
            }
        }
        tokio::time::sleep(backoff).await;
    }
}

async fn connect_channel(cfg: &AgentConnectConfig) -> CoreResult<Channel> {
    let endpoint = Channel::from_shared(cfg.controller.clone()).map_err(|e| {
        CoreError::InvalidArgument(format!("无效 controller URL: {e}"))
    })?;
    let is_https = cfg
        .controller
        .strip_prefix("https://")
        .is_some()
        || cfg.controller.starts_with("HTTPS://");
    let endpoint = if is_https {
        let tls = build_client_tls(&cfg.controller, &cfg.tls)?;
        endpoint
            .tls_config(tls)
            .map_err(|e| CoreError::Internal(format!("TLS 客户端配置失败: {e}")))?
    } else {
        if !cfg.controller.starts_with("http://") && !cfg.controller.starts_with("HTTP://") {
            return Err(CoreError::InvalidArgument(
                "controller URL 须以 http:// 或 https:// 开头".into(),
            ));
        }
        endpoint
    };
    endpoint
        .connect()
        .await
        .map_err(|e| CoreError::Internal(format!("连接控制端失败: {e}")))
}

async fn run_session(state: &AppState, cfg: &AgentConnectConfig) -> SessionOutcome {
    info!(controller = %cfg.controller, "正在连接控制端");
    let channel = match connect_channel(cfg).await {
        Ok(c) => c,
        Err(e) => return SessionOutcome::HandshakeFailed(e),
    };

    let mut client = NodeServiceClient::new(channel);
    let (tx, rx) = mpsc::channel::<AgentSessionFrame>(64);
    let outbound = ReceiverStream::new(rx);
    let response = match client.agent_session(outbound).await {
        Ok(r) => r,
        Err(e) => {
            return SessionOutcome::HandshakeFailed(CoreError::Internal(format!(
                "打开 AgentSession 失败: {e}"
            )));
        }
    };
    let mut inbound = response.into_inner();

    let device_cred = match load_device_credential(&state.paths.root) {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "读取设备凭证失败，将尝试 enroll");
            None
        }
    };

    let handshake = AgentHandshakeRequest {
        enroll_token: if device_cred.is_none() {
            cfg.token.clone()
        } else {
            String::new()
        },
        device_credential: device_cred.unwrap_or_default(),
        node_id: state.node_id.clone(),
        core_version: env!("CARGO_PKG_VERSION").into(),
        capabilities: vec!["tunnel.unary".into()],
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        name: "astral-core".into(),
    };
    if tx
        .send(AgentSessionFrame {
            body: Some(agent_session_frame::Body::Handshake(handshake)),
        })
        .await
        .is_err()
    {
        return SessionOutcome::HandshakeFailed(CoreError::Internal("发送握手失败".into()));
    }

    let first = match inbound.next().await {
        Some(Ok(f)) => f,
        Some(Err(e)) => {
            return SessionOutcome::HandshakeFailed(CoreError::Internal(format!(
                "读握手响应失败: {e}"
            )));
        }
        None => {
            return SessionOutcome::HandshakeFailed(CoreError::Internal(
                "控制端关闭流（握手前）".into(),
            ));
        }
    };

    match first.body {
        Some(agent_session_frame::Body::HandshakeResult(res)) => {
            if res.controller_attestation.is_empty() {
                return SessionOutcome::HandshakeFailed(CoreError::FailedPrecondition(
                    "控制端未返回 attestation，拒绝建立会话".into(),
                ));
            }
            if res.controller_attestation != cfg.token {
                return SessionOutcome::HandshakeFailed(CoreError::FailedPrecondition(
                    "控制端 attestation 校验失败（密钥不匹配）".into(),
                ));
            }
            if !res.device_credential.is_empty() {
                if let Err(e) = save_device_credential(&state.paths.root, &res.device_credential) {
                    return SessionOutcome::HandshakeFailed(e);
                }
                info!("已保存设备凭证");
            }
            let nid = if res.node_id.is_empty() {
                state.node_id.clone()
            } else {
                res.node_id
            };
            info!(node_id = %nid, "Agent 握手成功");
        }
        Some(agent_session_frame::Body::Error(msg)) => {
            return SessionOutcome::HandshakeFailed(CoreError::FailedPrecondition(format!(
                "握手被拒绝: {msg}"
            )));
        }
        other => {
            return SessionOutcome::HandshakeFailed(CoreError::Internal(format!(
                "握手阶段收到非预期帧: {other:?}"
            )));
        }
    }

    let mut heartbeat = tokio::time::interval(cfg.heartbeat_interval);
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let body = async {
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    let frame = AgentSessionFrame {
                        body: Some(agent_session_frame::Body::Heartbeat(AgentHeartbeatRequest {
                            node_id: state.node_id.clone(),
                            instance_summaries: vec![],
                            ts: None,
                        })),
                    };
                    if tx.send(frame).await.is_err() {
                        return Err(CoreError::Internal("心跳发送失败（流已关闭）".into()));
                    }
                }
                frame = inbound.next() => {
                    match frame {
                        Some(Ok(f)) => {
                            if let Some(agent_session_frame::Body::TunnelRequest(req)) = f.body {
                                let (code, message, payload) = dispatch_tunnel_request(
                                    state,
                                    &req.full_method,
                                    &req.payload,
                                ).await;
                                let resp = AgentSessionFrame {
                                    body: Some(agent_session_frame::Body::TunnelResponse(
                                        AgentTunnelResponse {
                                            id: req.id,
                                            grpc_status: code,
                                            message,
                                            payload,
                                        },
                                    )),
                                };
                                if tx.send(resp).await.is_err() {
                                    return Err(CoreError::Internal(
                                        "回传隧道响应失败".into(),
                                    ));
                                }
                            }
                        }
                        Some(Err(e)) => {
                            return Err(CoreError::Internal(format!("读会话帧失败: {e}")));
                        }
                        None => return Ok(()),
                    }
                }
            }
        }
    };

    SessionOutcome::EstablishedThenEnded(body.await)
}
