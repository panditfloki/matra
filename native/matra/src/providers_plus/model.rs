//! Shapes shared by the parity readers. Everything here is serialised to the UI, so nothing here may
//! ever hold a credential.
use crate::usage::{LimitWindow, UsageSnapshot};
use serde::Serialize;
use serde_json::Value;

pub const LOCAL: &str = "localRuntime";
pub const USAGE: &str = "usage";

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LocalModel {
    pub name: String,
    pub memory_bytes: Option<i64>,
    /// "allocation": what the model occupies. "modelSize": the weights on disk, not a memory reading.
    pub memory_kind: String,
    pub gpu_memory_bytes: Option<i64>,
    pub context_length: Option<i64>,
    pub quantization: Option<String>,
    pub expires_at: Option<u64>,
    pub model_key: Option<String>,
    /// "processingPrompt" or "generating", only when the runtime itself says so.
    pub phase: Option<String>,
    pub queued: Option<i64>,
    pub tokens_per_second: Option<f64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LocalReading {
    pub models: Vec<LocalModel>,
    pub measures_speed: bool,
}

/// Server-wide measurements. A field the runtime did not report stays None and is never a zero.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Performance {
    pub generation_tokens_per_second: Option<f64>,
    pub active_requests: Option<i64>,
    pub queued_requests: Option<i64>,
    pub total_tokens: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ParitySnapshot {
    #[serde(flatten)]
    pub base: UsageSnapshot,
    pub display_name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_model: Option<LocalReading>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub performance: Option<Performance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
}

/// What a reader found. The engine turns it into a snapshot.
#[derive(Debug, Clone, Default)]
pub struct Reading {
    pub windows: Vec<LimitWindow>,
    pub plan: Option<String>,
    pub local_model: Option<LocalReading>,
    pub performance: Option<Performance>,
    pub details: Option<Value>,
    pub note: String,
    /// "stale" for a reading its source has stopped trusting. None means "ok".
    pub status: Option<&'static str>,
    /// When the source took the reading, if it was not just now.
    pub as_of: Option<u64>,
    /// A custom endpoint's own tracked totals, to be written back to settings.
    pub custom_tokens: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Failure {
    pub status: &'static str,
    /// User-readable and fixed by this code. Never a response body, URL query or credential.
    pub note: String,
    /// Seconds until the next attempt.
    pub retry: u64,
    /// The source said to slow down, so a persisted deadline applies.
    pub rate_limited: bool,
    /// Retry-After in seconds, which only ever raises the wait.
    pub retry_after: Option<u64>,
    /// A temporary failure, so the last reading stays on screen marked stale.
    pub transient: bool,
}

impl Failure {
    pub fn needs_auth(note: impl Into<String>) -> Self {
        Failure {
            status: "needsAuth",
            note: note.into(),
            retry: 300,
            rate_limited: false,
            retry_after: None,
            transient: false,
        }
    }
    /// A definite answer: the old reading must not stand in for it.
    pub fn error(note: impl Into<String>) -> Self {
        Failure {
            status: "error",
            note: note.into(),
            retry: 300,
            rate_limited: false,
            retry_after: None,
            transient: false,
        }
    }
    pub fn transient(note: impl Into<String>) -> Self {
        Failure {
            transient: true,
            ..Failure::error(note)
        }
    }
    pub fn rate_limited(note: impl Into<String>, retry_after: Option<u64>) -> Self {
        Failure {
            status: "backoff",
            note: note.into(),
            retry: 60,
            rate_limited: true,
            retry_after,
            transient: true,
        }
    }
    pub fn retry_in(mut self, secs: u64) -> Self {
        self.retry = secs;
        self
    }
}

pub fn fraction_window(id: &str, label: &str, used: f64, resets_at: Option<u64>) -> LimitWindow {
    LimitWindow {
        id: id.into(),
        label: label.into(),
        used: if used.is_finite() { used.clamp(0.0, 1.0) } else { 0.0 },
        resets_at,
        ..Default::default()
    }
}

/// A figure with no published denominator: the card shows the number and the ring only its track.
pub fn count_window(id: &str, label: &str, count: i64) -> LimitWindow {
    LimitWindow {
        id: id.into(),
        label: label.into(),
        count: Some(count),
        ..Default::default()
    }
}

/// A count this code can do arithmetic on: finite, sane, and never a boolean.
pub fn number(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite() && n.abs() < 1e15)
}

/// Whole-number JSON, also accepting the string form some of these servers answer with.
pub fn integer(v: &Value) -> Option<i64> {
    if let Some(n) = v.as_i64() {
        return Some(n);
    }
    if let Some(n) = number(v) {
        return (n.fract() == 0.0).then_some(n as i64);
    }
    v.as_str()?.trim().parse().ok()
}

pub fn rfc3339_ms(v: &Value) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .and_then(|d| u64::try_from(d.timestamp_millis()).ok())
}

pub fn non_empty(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
