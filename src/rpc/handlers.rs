//! JSON-RPC method 分发。

use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::app::AppState;
use crate::error::{CoreError, CoreResult};
use crate::model::{InstanceMeta, InstanceState};

#[derive(Deserialize, Default)]
struct InstanceIdParams {
    #[serde(default)]
    instance_id: String,
}

#[derive(Deserialize, Default)]
struct StartParams {
    #[serde(default)]
    toml: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    source_path: String,
}

#[derive(Deserialize, Default)]
struct ListMetaParams {
    #[serde(default)]
    instance_ids: Vec<String>,
}

#[derive(Deserialize, Default)]
struct LogsRecentParams {
    #[serde(default)]
    after: u64,
    #[serde(default = "default_log_limit")]
    limit: u32,
    #[serde(default)]
    instance_id: String,
}

fn default_log_limit() -> u32 {
    200
}

pub(super) async fn dispatch(state: &AppState, method: &str, params: Value) -> CoreResult<Value> {
    let params = if params.is_null() {
        json!({})
    } else {
        params
    };
    match method {
        "ping" => Ok(json!({ "ok": true })),
        "info" => Ok(json!({
            "api_version": "0.1.0",
            "core_version": state.runtime.core_version,
            "protocol": "astral.jsonrpc.v1",
        })),
        "instance.start" => instance_start(state, params).await,
        "instance.stop" => instance_stop(state, params),
        "instance.get" => instance_get(state, params).await,
        "instance.list_meta" => instance_list_meta(state, params).await,
        "network.status" => network_status(state, params).await,
        "logs.recent" => logs_recent(state, params),
        _ => Err(CoreError::MethodNotFound(method.into())),
    }
}

fn parse<T: for<'de> Deserialize<'de> + Default>(params: Value) -> CoreResult<T> {
    if params.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(params).map_err(|e| CoreError::InvalidArgument(e.to_string()))
}

fn parse_id(raw: &str) -> CoreResult<Uuid> {
    if raw.trim().is_empty() {
        return Err(CoreError::InvalidArgument("instance_id 为空".into()));
    }
    Uuid::parse_str(raw).map_err(|_| CoreError::InvalidArgument(format!("无效 instance_id: {raw}")))
}

async fn instance_start(state: &AppState, params: Value) -> CoreResult<Value> {
    let p: StartParams = parse(params)?;
    if p.toml.trim().is_empty() {
        return Err(CoreError::InvalidArgument("缺少 toml".into()));
    }
    let id = state
        .engine
        .start_toml(&p.toml, &p.display_name, &p.source_path)?;
    let mut summary = state.engine.summary_of(id).await;
    if !p.display_name.is_empty() {
        summary.display_name = p.display_name;
    }
    Ok(json!({
        "instance_id": id.to_string(),
        "state": summary.state,
        "summary": summary,
    }))
}

fn instance_stop(state: &AppState, params: Value) -> CoreResult<Value> {
    let p: InstanceIdParams = parse(params)?;
    let id = parse_id(&p.instance_id)?;
    state.engine.stop(id)?;
    Ok(json!({
        "instance_id": p.instance_id,
        "state": InstanceState::Stopped,
    }))
}

async fn instance_get(state: &AppState, params: Value) -> CoreResult<Value> {
    let p: InstanceIdParams = parse(params)?;
    let id = parse_id(&p.instance_id)?;
    if !state.engine.exists(id) && state.engine.cache().get_uuid(id)?.is_none() {
        return Err(CoreError::NotFound(format!("实例不存在: {}", p.instance_id)));
    }
    let summary = state.engine.summary_of(id).await;
    let cached = state.engine.cache().get_uuid(id)?;
    Ok(json!({
        "summary": summary,
        "source_path": cached.map(|c| c.source_path).unwrap_or_default(),
    }))
}

async fn instance_list_meta(state: &AppState, params: Value) -> CoreResult<Value> {
    let p: ListMetaParams = parse(params)?;
    let mut metas = Vec::new();
    for s in state.engine.list_summaries().await {
        if !p.instance_ids.is_empty() && !p.instance_ids.contains(&s.instance_id) {
            continue;
        }
        let cached = state.engine.cache().get(&s.instance_id)?;
        metas.push(InstanceMeta {
            instance_id: s.instance_id,
            display_name: s.display_name,
            state: s.state,
            running: s.running,
            source_path: cached.map(|c| c.source_path).unwrap_or_default(),
            error_message: s.error_message,
            started_at_unix_ms: s.started_at_unix_ms,
        });
    }
    Ok(json!({ "metas": metas }))
}

async fn network_status(state: &AppState, params: Value) -> CoreResult<Value> {
    let p: InstanceIdParams = parse(params)?;
    let id = parse_id(&p.instance_id)?;
    if !state.engine.exists(id) {
        return Err(CoreError::NotFound(format!("实例不存在: {}", p.instance_id)));
    }
    let summary = state.engine.summary_of(id).await;
    let peers = state.engine.list_peers(id).await;
    let (my_ipv4, my_ipv6, hostname) = state.engine.local_addrs(id).await;
    Ok(json!({
        "instance_id": p.instance_id,
        "running": summary.running,
        "state": summary.state,
        "error_message": summary.error_message,
        "dev_name": summary.dev_name,
        "my_ipv4": my_ipv4,
        "my_ipv6": my_ipv6,
        "hostname": hostname,
        "network_name": summary.network_name,
        "peer_count": peers.len() as u32,
        "peers": peers,
    }))
}

fn logs_recent(state: &AppState, params: Value) -> CoreResult<Value> {
    let p: LogsRecentParams = parse(params)?;
    let limit = if p.limit == 0 { 200 } else { p.limit.min(2000) } as usize;
    let lines = if p.instance_id.trim().is_empty() {
        state.logs.recent_since(p.after, limit)
    } else {
        state.logs.recent_since_for_instance(p.after, limit, p.instance_id.trim())
    };
    let last_seq = lines.last().map(|l| l.seq).unwrap_or(p.after);
    Ok(json!({ "lines": lines, "last_seq": last_seq }))
}
