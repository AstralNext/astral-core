//! 进程内日志中枢：tracing 采集、环形缓冲。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::fmt;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer;

/// 单行日志。
#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    /// 单调序号，供 `logs.recent(after)` 去重。
    pub seq: u64,
    /// Unix 秒。
    pub unix_secs: i64,
    /// 级别。
    pub level: String,
    /// tracing target。
    pub target: String,
    /// 正文。
    pub message: String,
    /// 关联实例（可空）。
    pub instance_id: String,
}

const RING_CAP: usize = 2000;

static SINKS: Mutex<Vec<Weak<LogHubInner>>> = Mutex::new(Vec::new());
static INSTALLED: OnceLock<()> = OnceLock::new();

struct LogHubInner {
    recent: Mutex<VecDeque<LogLine>>,
    seq: AtomicU64,
}

/// 可克隆的日志中枢（每个 Core 进程一份，测试里可并存）。
#[derive(Clone)]
pub struct LogHub {
    inner: std::sync::Arc<LogHubInner>,
}

impl LogHub {
    /// 创建中枢并尽量安装全局 tracing（fmt + 采集层）。多次调用时后续 `try_init` 会被忽略。
    pub fn install(filter: &str) -> Self {
        let hub = Self {
            inner: std::sync::Arc::new(LogHubInner {
                recent: Mutex::new(VecDeque::with_capacity(RING_CAP)),
                seq: AtomicU64::new(0),
            }),
        };
        if let Ok(mut sinks) = SINKS.lock() {
            sinks.retain(|w| w.strong_count() > 0);
            sinks.push(std::sync::Arc::downgrade(&hub.inner));
        }
        let _ = INSTALLED.get_or_init(|| {
            let env = EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info"));
            let _ = tracing_subscriber::registry()
                .with(env)
                .with(fmt::layer().with_target(true))
                .with(CaptureLayer)
                .try_init();
        });
        hub
    }

    /// 环形缓冲中 `seq > after` 的日志；`after=0` 时返回最近 `limit` 条。
    pub fn recent_since(&self, after: u64, limit: usize) -> Vec<LogLine> {
        let limit = limit.clamp(1, 2000);
        let Ok(buf) = self.inner.recent.lock() else {
            return Vec::new();
        };
        if after == 0 {
            return buf
                .iter()
                .rev()
                .take(limit)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
        }
        buf.iter()
            .filter(|l| l.seq > after)
            .take(limit)
            .cloned()
            .collect()
    }

    fn push(inner: &LogHubInner, mut line: LogLine) {
        line.seq = inner.seq.fetch_add(1, Ordering::Relaxed) + 1;
        if let Ok(mut buf) = inner.recent.lock() {
            if buf.len() >= RING_CAP {
                buf.pop_front();
            }
            buf.push_back(line);
        }
    }
}

struct CaptureLayer;

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut vis = FieldCollector::default();
        event.record(&mut vis);
        let line = LogLine {
            seq: 0,
            unix_secs: now_unix_secs(),
            level: format!("{}", event.metadata().level()).to_ascii_lowercase(),
            target: event.metadata().target().to_string(),
            message: vis.message,
            instance_id: vis.instance_id,
        };
        if let Ok(mut sinks) = SINKS.lock() {
            sinks.retain(|w| w.strong_count() > 0);
            for weak in sinks.iter() {
                if let Some(inner) = weak.upgrade() {
                    LogHub::push(&inner, line.clone());
                }
            }
        }
    }
}

#[derive(Default)]
struct FieldCollector {
    message: String,
    instance_id: String,
}

impl FieldCollector {
    fn put(&mut self, name: &str, value: String) {
        if name == "message" {
            self.message = value;
        } else if name == "instance_id" {
            self.instance_id = value;
        }
    }
}

impl Visit for FieldCollector {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field.name(), format!("{value:?}").trim_matches('"').to_string());
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field.name(), value.to_string());
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field.name(), value.to_string());
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field.name(), value.to_string());
    }

    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field.name(), value.to_string());
    }
}

fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
