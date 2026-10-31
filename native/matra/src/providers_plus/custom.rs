//! User-defined endpoints speaking the OpenAI, Anthropic or Gemini protocol: model discovery, health
//! and usage. Usage comes from a preset or a mapped JSON endpoint, or from figures the user typed in.
//! A figure nobody reported is left out rather than shown as zero.
use super::metrics;
use super::model::{count_window, fraction_window, Failure, Reading};
use super::net::{self, Reply};
use crate::usage::LimitWindow;
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

const MAX_MODELS: usize = 200;
const MAX_MODEL_ID: usize = 200;
pub const MAX_ENDPOINTS: usize = 64;
const MAX_MAPPING: usize = 64 * 1024;
const MAX_FIELD: usize = 128;

fn default_header() -> String {
    "Authorization".into()
}
fn default_true() -> bool {
    true
}
fn default_api() -> String {
    "openAICompatible".into()
}
fn default_source() -> String {
    "manual".into()
}
fn default_auth() -> String {
    "apiKey".into()
}
fn default_unit() -> String {
    "currency".into()
}
fn default_health() -> String {
    "idle".into()
}
fn default_accent() -> String {
    "#6366F1".into()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UsageDay {
    pub day: String,
    pub total_tokens: i64,
}

/// Deliberately has no key field. A key arriving in JSON is ignored by serde and cannot reach the file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomEndpoint {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(default = "default_header")]
    pub header_key: String,
    #[serde(default = "default_api")]
    pub api_type: String,
    #[serde(default)]
    pub selected_model: String,
    #[serde(default)]
    pub available_models: Vec<String>,
    #[serde(default = "default_true")]
    pub is_enabled: bool,
    #[serde(default = "default_accent")]
    pub accent_color_hex: String,
    #[serde(default)]
    pub icon_preset: Option<String>,
    #[serde(default = "default_source")]
    pub usage_source: String,
    #[serde(default)]
    pub usage_preset: Option<String>,
    #[serde(default)]
    pub usage_url: Option<String>,
    #[serde(default)]
    pub usage_records_path: Option<String>,
    #[serde(default)]
    pub usage_model_field: Option<String>,
    #[serde(default)]
    pub usage_token_field: Option<String>,
    #[serde(default)]
    pub usage_model_filter: Option<String>,
    #[serde(default = "default_auth")]
    pub usage_authentication: String,
    #[serde(default)]
    pub usage_history: Vec<UsageDay>,
    #[serde(default = "default_unit")]
    pub tracking_unit: String,
    #[serde(default)]
    pub monthly_budget_usd: Option<f64>,
    #[serde(default)]
    pub current_spend_usd: Option<f64>,
    #[serde(default)]
    pub monthly_budget_tokens_m: Option<f64>,
    #[serde(default)]
    pub current_tokens_used_m: Option<f64>,
    #[serde(default)]
    pub display_remaining: bool,
    #[serde(default)]
    pub show_currency: bool,
    #[serde(default)]
    pub last_latency_ms: Option<u64>,
    #[serde(default = "default_health")]
    pub last_health_status: String,
    #[serde(default)]
    pub last_checked_at: Option<u64>,
    /// Output only: filled from the store when listed, and ignored on input.
    #[serde(default)]
    pub has_key: bool,
}

fn field_ok(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|v| v.len() <= MAX_FIELD && !v.chars().any(char::is_control))
}

fn amount_ok(value: Option<f64>) -> bool {
    value.is_none_or(|v| v.is_finite() && v >= 0.0)
}

/// Normalises a draft and returns the first problem in words the settings form can show.
pub fn validate(mut e: CustomEndpoint) -> Result<CustomEndpoint, String> {
    e.name = e.name.trim().to_string();
    if e.name.is_empty() || e.name.chars().count() > 120 || e.name.chars().any(char::is_control) {
        return Err("Give this endpoint a name of up to 120 characters.".into());
    }
    e.base_url = e.base_url.trim().to_string();
    net::parse_base(&e.base_url)
        .map_err(|_| "The address must start with http:// or https://, with no user name, query or fragment.".to_string())?;
    e.header_key = e.header_key.trim().to_string();
    if e.header_key.is_empty() {
        e.header_key = default_header();
    }
    if !net::header_name_ok(&e.header_key) {
        return Err("That header name cannot carry a key.".into());
    }
    if !matches!(e.api_type.as_str(), "openAICompatible" | "anthropic" | "google") {
        return Err("Choose the OpenAI, Anthropic or Gemini protocol.".into());
    }
    if !matches!(e.usage_source.as_str(), "manual" | "jsonEndpoint") {
        return Err("Unknown usage source.".into());
    }
    if !matches!(e.usage_authentication.as_str(), "apiKey" | "none") {
        return Err("Unknown usage authentication.".into());
    }
    if !matches!(e.tracking_unit.as_str(), "currency" | "tokens") {
        return Err("Track either currency or tokens.".into());
    }
    if let Some(preset) = e.usage_preset.as_deref() {
        if !matches!(preset, "litellm" | "openRouter" | "newAPI" | "vllm" | "llamaCpp" | "abacus") {
            return Err("Unknown usage preset.".into());
        }
    }
    if ![&e.usage_records_path, &e.usage_model_field, &e.usage_token_field, &e.usage_model_filter]
        .into_iter()
        .all(field_ok)
    {
        return Err("Field paths must be at most 128 characters.".into());
    }
    if let Some(usage) = e.usage_url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
        net::parse_base(usage).map_err(|_| "The usage address must be an http or https URL with no user name, query or fragment.".to_string())?;
        if net::origin(usage) != net::origin(&e.base_url) {
            return Err("The usage address must match the base address's scheme, host and port.".into());
        }
        e.usage_url = Some(usage.to_string());
    } else {
        e.usage_url = None;
    }
    if ![e.monthly_budget_usd, e.current_spend_usd, e.monthly_budget_tokens_m, e.current_tokens_used_m]
        .into_iter()
        .all(amount_ok)
    {
        return Err("Budgets and usage must be zero or more.".into());
    }
    e.available_models.retain(|m| !m.is_empty() && m.len() <= MAX_MODEL_ID);
    e.available_models.truncate(MAX_MODELS);
    if e.selected_model.len() > MAX_MODEL_ID {
        return Err("That model name is too long.".into());
    }
    if e.usage_history.len() > 31 {
        let keep = e.usage_history.len() - 31;
        e.usage_history.drain(..keep);
    }
    e.has_key = false;
    Ok(e)
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// A local name, unique enough for a settings list. Not a secret and not used for anything else.
pub fn new_id(now: u64) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(now);
    format!("{now:x}-{:016x}", hasher.finish())
}

pub fn auth_headers(api_type: &str, header_key: &str, key: &str) -> Vec<(String, String)> {
    let Some(key) = net::clean_secret(key) else {
        return vec![];
    };
    match api_type {
        "anthropic" => vec![net::header("x-api-key", key), net::header("anthropic-version", "2023-06-01")],
        "google" => vec![net::header("x-goog-api-key", key)],
        _ => {
            let name = if header_key.trim().is_empty() { "Authorization" } else { header_key.trim() };
            if name.eq_ignore_ascii_case("authorization") && !key.to_lowercase().starts_with("bearer ") {
                vec![net::header("Authorization", format!("Bearer {key}"))]
            } else {
                vec![net::header(name, key)]
            }
        }
    }
}

pub fn models_url(base: &str, api_type: &str) -> Option<String> {
    let mut url = net::parse_base(base).ok()?;
    let path = url.path().trim_matches('/').to_string();
    if path.ends_with("models") {
        return Some(url.to_string());
    }
    let join = |parts: &[&str]| format!("/{}", parts.iter().filter(|p| !p.is_empty()).copied().collect::<Vec<_>>().join("/"));
    let next = match api_type {
        "anthropic" if path.ends_with("v1") => join(&[&path, "models"]),
        "anthropic" => join(&[&path, "v1", "models"]),
        "google" if path.ends_with("v1beta") => join(&[&path, "models"]),
        "google" => join(&[&path, "v1beta", "models"]),
        _ => join(&[&path, "models"]),
    };
    url.set_path(&next);
    Some(url.to_string())
}

pub fn parse_models(root: &Value, api_type: &str) -> Vec<String> {
    let ids = |list: &Value, keys: &[&str]| -> Vec<String> {
        list.as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| keys.iter().find_map(|k| row[*k].as_str()).map(str::to_owned))
            .collect()
    };
    let mut models = match api_type {
        "anthropic" => ids(&root["data"], &["id"]),
        "google" => ids(&root["models"], &["name"])
            .into_iter()
            .map(|n| n.strip_prefix("models/").map(str::to_owned).unwrap_or(n))
            .collect(),
        _ if root["data"].is_array() => ids(&root["data"], &["id"]),
        _ => ids(&root["models"], &["name", "id"]),
    };
    models.sort();
    models.dedup();
    models.retain(|m| !m.is_empty() && m.len() <= MAX_MODEL_ID);
    models.truncate(MAX_MODELS);
    models
}

#[derive(Debug, Clone, Serialize)]
pub struct TestResult {
    pub health: String,
    pub latency_ms: u64,
    pub models: Vec<String>,
    pub error: Option<String>,
    #[serde(skip)]
    pub http_status: Option<u16>,
}

fn unreachable(latency_ms: u64, error: String, http_status: Option<u16>) -> TestResult {
    TestResult {
        health: "unreachable".into(),
        latency_ms,
        models: vec![],
        error: Some(error),
        http_status,
    }
}

const PLAIN_HTTP_KEY: &str = "A key cannot be sent over plain HTTP to a public address. Use https or a private address.";

/// Reachability, authentication and model discovery in one request. Failure text names a category only:
/// the endpoint's own response can hold private server details and is not kept.
pub fn test_endpoint(endpoint: &CustomEndpoint, key: Option<&str>) -> TestResult {
    if net::parse_base(&endpoint.base_url).is_err() {
        return unreachable(0, "The address must start with http:// or https://".into(), None);
    }
    let Some(url) = models_url(&endpoint.base_url, &endpoint.api_type) else {
        return unreachable(0, "Could not build the model-list URL".into(), None);
    };
    let key = key.filter(|k| !k.trim().is_empty());
    if key.is_some() && !net::key_allowed(&url) {
        return unreachable(0, PLAIN_HTTP_KEY.into(), None);
    }
    let headers = key.map_or(vec![], |k| auth_headers(&endpoint.api_type, &endpoint.header_key, k));
    let started = Instant::now();
    let reply = net::get(&url, &headers, net::ENDPOINT_TIMEOUT);
    let latency = (started.elapsed().as_millis() as u64).max(1);
    match reply {
        Err(net::NetError::Timeout) => unreachable(latency, "The endpoint did not answer in time".into(), None),
        Err(_) => unreachable(latency, "Could not reach the endpoint".into(), None),
        Ok(reply) if !(200..300).contains(&reply.status) => unreachable(
            latency,
            format!("The endpoint answered {}", reply.status),
            Some(reply.status),
        ),
        Ok(reply) => TestResult {
            health: if latency > 800 { "slow" } else { "online" }.into(),
            latency_ms: latency,
            models: serde_json::from_slice::<Value>(&reply.body)
                .map(|root| parse_models(&root, &endpoint.api_type))
                .unwrap_or_default(),
            error: None,
            http_status: Some(reply.status),
        },
    }
}

pub fn preset_url(preset: &str, base: &str) -> Option<String> {
    let mut url = net::parse_base(base).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    match preset {
        // Only these hosts carry these routes. Never send a vendor-shaped request anywhere else.
        "openRouter" => {
            if url.scheme() != "https" || host != "openrouter.ai" || url.port().is_some() || !matches!(url.path(), "/api/v1" | "/api/v1/") {
                return None;
            }
            url.set_path("/api/v1/key");
        }
        "abacus" => {
            if url.scheme() != "https" || host != "routellm.abacus.ai" || url.port().is_some() {
                return None;
            }
            url.set_path("/api/v0/_getOrganizationComputePoints");
        }
        _ => {
            let suffix = match preset {
                "litellm" => "/key/info",
                "newAPI" => "/api/usage/token",
                "vllm" | "llamaCpp" => "/metrics",
                _ => return None,
            };
            let mut path = url.path().trim_end_matches('/').to_string();
            if path == "/v1" {
                path.clear();
            } else if let Some(stripped) = path.strip_suffix("/v1") {
                path = stripped.to_string();
            }
            url.set_path(&format!("{path}{suffix}"));
        }
    }
    Some(url.to_string())
}

/// `(primary window plus extras, detail text per window, performance, spend period)`.
pub struct PresetReading {
    pub windows: Vec<LimitWindow>,
    pub texts: Value,
    pub performance: Option<super::model::Performance>,
    pub spend_period: Option<&'static str>,
}

fn format_tokens_m(millions: f64) -> String {
    if millions >= 1000.0 {
        format!("{:.1}B", millions / 1000.0)
    } else if millions >= 10.0 {
        format!("{millions:.0}M")
    } else if millions >= 1.0 {
        let rounded = (millions * 10.0).round() / 10.0;
        if rounded.fract() == 0.0 { format!("{rounded:.0}M") } else { format!("{rounded:.1}M") }
    } else if millions > 0.0 {
        let k = (millions * 10_000.0).round() / 10.0;
        if k.fract() == 0.0 { format!("{k:.0}k") } else { format!("{k:.1}k") }
    } else {
        "0M".into()
    }
}

fn tokens_text(tokens: i64) -> String {
    if tokens > 0 && tokens < 1000 {
        tokens.to_string()
    } else {
        format_tokens_m(tokens as f64 / 1_000_000.0)
    }
}

fn format_credits(value: f64) -> String {
    let v = value.max(0.0);
    if v < 1000.0 {
        return format!("{:.0}", v.round());
    }
    let k = (v / 100.0).round() / 10.0;
    if k.fract() == 0.0 { format!("{k:.0}K") } else { format!("{k:.1}K") }
}

pub fn parse_preset(preset: &str, body: &[u8]) -> Option<PresetReading> {
    let json = || serde_json::from_slice::<Value>(body).ok();
    let money = |id: &str, label: &str, amount: f64, period: &'static str| PresetReading {
        windows: vec![count_window(id, label, amount.round() as i64)],
        texts: json!({ id: {"used_text": format!("${amount:.2}"), "detail": format!("${amount:.2}")} }),
        performance: None,
        spend_period: Some(period),
    };
    match preset {
        "litellm" => {
            let spend = json()?["info"]["spend"].as_f64().filter(|v| v.is_finite() && *v >= 0.0)?;
            Some(money("preset-spend", "Total Spend (USD)", spend, "lifetime"))
        }
        "openRouter" => {
            let spend = json()?["data"]["usage_monthly"].as_f64().filter(|v| v.is_finite() && *v >= 0.0)?;
            Some(money("preset-spend", "Spend This Month (USD)", spend, "month"))
        }
        "newAPI" => {
            let root = json()?;
            let data = &root["data"];
            if data["object"].as_str() != Some("token_usage") {
                return None;
            }
            let used = data["total_used"].as_i64().filter(|v| *v >= 0)?;
            let granted = match data.get("total_granted").filter(|v| !v.is_null()) {
                Some(v) => Some(v.as_i64().filter(|g| *g >= 0)?),
                None => None,
            };
            let granted = if data["unlimited_quota"].as_bool() == Some(true) { None } else { granted };
            let window = match granted.filter(|g| *g > 0) {
                Some(g) => fraction_window("preset-quota", "Quota Used", used as f64 / g as f64, None),
                None => count_window("preset-quota", "Quota Used", used),
            };
            let detail = granted.filter(|g| *g > 0).map_or(used.to_string(), |g| format!("{used} / {g}"));
            Some(PresetReading {
                windows: vec![window],
                texts: json!({"preset-quota": {"used_text": used.to_string(), "detail": detail}}),
                performance: None,
                spend_period: None,
            })
        }
        "vllm" => {
            let tokens = metrics::vllm(std::str::from_utf8(body).ok()?)?;
            let text = tokens_text(tokens);
            Some(PresetReading {
                windows: vec![count_window("preset-tokens", "Tokens Since Server Start", tokens)],
                texts: json!({"preset-tokens": {"used_text": text, "detail": format!("{text} tokens")}}),
                performance: Some(super::model::Performance {
                    total_tokens: Some(tokens),
                    ..Default::default()
                }),
                spend_period: None,
            })
        }
        "llamaCpp" => {
            let m = metrics::llama(std::str::from_utf8(body).ok()?)?;
            let mut windows = Vec::new();
            if let Some(rate) = m.tokens_per_second {
                windows.push(count_window("llamacpp-speed", "Average generation speed (tok/s)", rate.round() as i64));
            }
            if let Some(active) = m.active {
                windows.push(count_window("llamacpp-active", "Active requests", active));
            }
            if let Some(queued) = m.queued {
                windows.push(count_window("llamacpp-queued", "Queued requests", queued));
            }
            windows.push(count_window("preset-tokens", "Tokens Since Server Start", m.total_tokens));
            Some(PresetReading {
                windows,
                texts: json!({"preset-tokens": {"used_text": tokens_text(m.total_tokens)}}),
                performance: Some(super::model::Performance {
                    generation_tokens_per_second: m.tokens_per_second,
                    active_requests: m.active,
                    queued_requests: m.queued,
                    total_tokens: Some(m.total_tokens),
                }),
                spend_period: None,
            })
        }
        "abacus" => {
            let root = json()?;
            if root["success"].as_bool() != Some(true) {
                return None;
            }
            let r = &root["result"];
            let finite = |v: &Value| v.as_f64().filter(|n| n.is_finite());
            let left = finite(&r["computePointsLeft"]).filter(|v| *v >= 0.0)?;
            let total = finite(&r["totalComputePoints"]).filter(|v| *v >= 0.0)?;
            let users = finite(&r["userCount"]).unwrap_or(1.0).max(1.0);
            let monthly = finite(&r["normalMonthlyCredits"])? * users;
            if !(monthly.is_finite() && monthly > 0.0) {
                return None;
            }
            let total = total.max(monthly);
            // The ring tracks the monthly allowance. Credits bought on top are reported beside it.
            let used = (monthly - left.min(monthly)).max(0.0);
            let extra = total - monthly;
            let detail = if extra >= 1.0 {
                format!(
                    "{} credits left, {} monthly + {} extra",
                    format_credits(left),
                    format_credits(monthly),
                    format_credits(extra)
                )
            } else {
                format!("{} of {} monthly credits left", format_credits(left), format_credits(monthly))
            };
            Some(PresetReading {
                windows: vec![fraction_window("preset-credits", "Credits", used / monthly, None)],
                texts: json!({"preset-credits": {"used_text": format!("{} left", format_credits(left)), "detail": detail}}),
                performance: None,
                spend_period: None,
            })
        }
        _ => None,
    }
}

fn value_at<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return Some(root);
    }
    path.split('.').try_fold(root, |value, part| match value {
        Value::Object(map) => map.get(part),
        Value::Array(list) => list.get(part.parse::<usize>().ok()?),
        _ => None,
    })
}

/// Total tokens in millions. An empty record list is a real zero; a filter that matches nothing, or a
/// row without a usable count, means the mapping does not fit and is not a reading.
pub fn parse_json_usage(body: &[u8], records: &str, model_field: &str, token_field: &str, filter: Option<&str>) -> Option<f64> {
    let root: Value = serde_json::from_slice(body).ok()?;
    let rows = value_at(&root, records)?.as_array()?;
    if rows.is_empty() {
        return Some(0.0);
    }
    if !rows.iter().all(Value::is_object) {
        return None;
    }
    let filter = filter.filter(|f| !f.is_empty());
    let selected: Vec<&Value> = rows
        .iter()
        .filter(|row| filter.is_none_or(|f| value_at(row, model_field).and_then(Value::as_str) == Some(f)))
        .collect();
    if selected.is_empty() {
        return None;
    }
    let mut total = 0.0;
    for row in selected {
        let tokens = value_at(row, token_field)?.as_f64()?;
        if !tokens.is_finite() || tokens < 0.0 {
            return None;
        }
        total += tokens;
    }
    Some(total / 1_000_000.0)
}

#[derive(Debug, Clone, Serialize)]
pub struct Mapping {
    pub usage_url: String,
    pub usage_records_path: String,
    pub usage_model_field: String,
    pub usage_token_field: String,
    pub usage_model_filter: Option<String>,
}

pub fn import_mapping(base: &str, text: &str) -> Result<Mapping, String> {
    if text.len() > MAX_MAPPING {
        return Err("File exceeds maximum size of 64 KiB.".into());
    }
    let root: Value = serde_json::from_str(text).map_err(|_| "File contains malformed JSON.".to_string())?;
    let object = root.as_object().ok_or("File contains malformed JSON.")?;
    const KNOWN: [&str; 7] = ["version", "unit", "usageURL", "recordsPath", "modelField", "tokenField", "modelFilter"];
    if object.keys().any(|k| !KNOWN.contains(&k.as_str())) {
        return Err("File contains unrecognized fields.".into());
    }
    if root["version"].as_i64() != Some(1) {
        return Err("Unsupported format version; only version 1 is supported.".into());
    }
    if root["unit"].as_str() != Some("tokens") {
        return Err("Unsupported unit; only 'tokens' is supported.".into());
    }
    let string = |key: &str| root[key].as_str().map(str::to_owned);
    let (Some(usage), Some(records), Some(model), Some(token)) =
        (string("usageURL"), string("recordsPath"), string("modelField"), string("tokenField"))
    else {
        return Err("File contains malformed JSON.".into());
    };
    let short = |v: &str| v.chars().count() <= MAX_FIELD;
    let filter = string("modelFilter");
    if !short(&records) || model.is_empty() || !short(&model) || token.is_empty() || !short(&token) || !filter.as_deref().is_none_or(short) {
        return Err("Field paths must be non-empty and at most 128 characters.".into());
    }
    let usage = usage.trim().to_string();
    let invalid = "Usage URL must be an http or https URL without userinfo, query, or fragment.";
    net::parse_base(&usage).map_err(|_| invalid.to_string())?;
    net::parse_base(base).map_err(|_| invalid.to_string())?;
    if net::origin(&usage) != net::origin(base) {
        return Err("Usage URL must match the base URL's scheme, host, and port.".into());
    }
    Ok(Mapping {
        usage_url: usage,
        usage_records_path: records,
        usage_model_field: model,
        usage_token_field: token,
        usage_model_filter: filter,
    })
}

fn fetch(url: &str, headers: &[(String, String)]) -> Result<Reply, Failure> {
    net::get(url, headers, net::ENDPOINT_TIMEOUT).map_err(|_| Failure::transient("Could not reach the endpoint.").retry_in(60))
}

fn checked(reply: Reply) -> Result<Reply, Failure> {
    match reply.status {
        200..=299 => Ok(reply),
        401 | 403 => Err(Failure::needs_auth("The endpoint refused this key. Check it in Settings.")),
        429 => Err(Failure::rate_limited(
            "The endpoint asked to slow down. Waiting before retrying.",
            reply.retry_after,
        )),
        code => Err(Failure::transient(format!("The endpoint answered {code}.")).retry_in(60)),
    }
}

fn next_month_ms(now_ms: u64) -> Option<u64> {
    use chrono::{Local, TimeZone};
    let now = Local.timestamp_millis_opt(now_ms as i64).single()?;
    let (year, month) = if now.month() == 12 { (now.year() + 1, 1) } else { (now.year(), now.month() + 1) };
    let next = Local.with_ymd_and_hms(year, month, 1, 0, 0, 0).earliest()?;
    u64::try_from(next.timestamp_millis()).ok()
}

/// What the user tracks by hand or by a mapped endpoint. `used` is always the spent fraction, so band
/// colours keep their meaning, and `display_remaining` is reported for the card to word it.
fn tracking_windows(e: &CustomEndpoint, tokens_m: Option<f64>, now: u64) -> (Vec<LimitWindow>, Value) {
    let reset = next_month_ms(now);
    if e.tracking_unit == "tokens" {
        let used = tokens_m.or(e.current_tokens_used_m);
        return match (used, e.monthly_budget_tokens_m.filter(|b| *b > 0.0)) {
            (Some(used), Some(budget)) => {
                let label = if e.display_remaining { "Remaining Tokens" } else { "Monthly Tokens" };
                let detail = if e.display_remaining {
                    format!("{} / {} tokens remaining", format_tokens_m((budget - used).max(0.0)), format_tokens_m(budget))
                } else {
                    format!("{} / {} tokens", format_tokens_m(used), format_tokens_m(budget))
                };
                (
                    vec![fraction_window("token-budget", label, used / budget, reset)],
                    json!({"token-budget": {"used_text": format_tokens_m(used), "detail": detail}}),
                )
            }
            (Some(used), None) => {
                let tokens = (used * 1_000_000.0).round() as i64;
                (
                    vec![count_window("token-tracking", "Tokens Used", tokens)],
                    json!({"token-tracking": {"used_text": format_tokens_m(used), "detail": format!("{} tokens", format_tokens_m(used))}}),
                )
            }
            _ => (vec![], json!({})),
        };
    }
    match (e.current_spend_usd, e.monthly_budget_usd.filter(|b| *b > 0.0)) {
        (Some(spend), Some(budget)) => {
            let label = if e.display_remaining { "Remaining Budget" } else { "Monthly Budget" };
            let shown = if e.display_remaining { (budget - spend).max(0.0) } else { spend };
            let detail = if e.display_remaining {
                format!("${shown:.2} / ${budget:.2} remaining")
            } else {
                format!("${shown:.2} / ${budget:.2}")
            };
            (
                vec![fraction_window("monthly-budget", label, spend / budget, reset)],
                json!({"monthly-budget": {"used_text": format!("${shown:.2}"), "detail": detail}}),
            )
        }
        (Some(spend), None) => (
            vec![count_window("spend-tracking", "Total Spend (USD)", spend.round() as i64)],
            json!({"spend-tracking": {"used_text": format!("${spend:.2}"), "detail": format!("${spend:.2}")}}),
        ),
        _ => (vec![], json!({})),
    }
}

pub fn read(e: &CustomEndpoint, key: Option<&str>, now: u64) -> Result<Reading, Failure> {
    if net::parse_base(&e.base_url).is_err() {
        return Err(Failure::error("The address must start with http:// or https://."));
    }
    if e.usage_source == "jsonEndpoint" {
        if let Some(preset) = e.usage_preset.as_deref() {
            return read_preset(e, preset, key);
        }
    }
    let usage_key = if e.usage_source == "jsonEndpoint" && e.usage_authentication == "none" { None } else { key };
    let mut usage_tokens = None;
    if e.usage_source == "jsonEndpoint" && e.tracking_unit == "tokens" {
        if let (Some(url), Some(records), Some(model), Some(token)) = (
            e.usage_url.as_deref(),
            e.usage_records_path.as_deref(),
            e.usage_model_field.as_deref(),
            e.usage_token_field.as_deref(),
        ) {
            if usage_key.is_some() && !net::key_allowed(url) {
                return Err(Failure::error(PLAIN_HTTP_KEY));
            }
            let headers = usage_key.map_or(vec![], |k| auth_headers(&e.api_type, &e.header_key, k));
            let reply = checked(fetch(url, &headers)?)?;
            usage_tokens = Some(
                parse_json_usage(&reply.body, records, model, token, e.usage_model_filter.as_deref())
                    .ok_or_else(|| Failure::transient("The usage endpoint did not match its field mapping.").retry_in(60))?,
            );
        }
    }
    let probe = test_endpoint(e, usage_key);
    if probe.health == "unreachable" && usage_tokens.is_none() {
        return Err(match probe.http_status {
            Some(401 | 403) => Failure::needs_auth("The endpoint refused this key. Check it in Settings."),
            Some(429) => Failure::rate_limited("The endpoint asked to slow down. Waiting before retrying.", None),
            _ => Failure::transient(probe.error.clone().unwrap_or_else(|| "Could not reach the endpoint.".into())).retry_in(60),
        });
    }
    let (windows, texts) = tracking_windows(e, usage_tokens, now);
    let note = if windows.is_empty() {
        "Online. No usage is tracked for this endpoint yet.".to_string()
    } else {
        String::new()
    };
    Ok(Reading {
        windows,
        note,
        custom_tokens: usage_tokens,
        details: Some(json!({
            "health": probe.health,
            "latency_ms": probe.latency_ms,
            "usage_history": e.usage_history,
            "display_remaining": e.display_remaining,
            "texts": texts,
        })),
        ..Default::default()
    })
}

fn read_preset(e: &CustomEndpoint, preset: &str, key: Option<&str>) -> Result<Reading, Failure> {
    let url = preset_url(preset, &e.base_url).ok_or_else(|| Failure::error("This preset does not work with this address."))?;
    let key = if e.usage_authentication == "none" { None } else { key };
    if key.is_some() && !net::key_allowed(&url) {
        return Err(Failure::error(PLAIN_HTTP_KEY));
    }
    let headers = key.map_or(vec![], |k| auth_headers(&e.api_type, &e.header_key, k));
    let reply = checked(fetch(&url, &headers)?)?;
    let parsed = parse_preset(preset, &reply.body)
        .ok_or_else(|| Failure::transient("The endpoint did not return this preset's usage format.").retry_in(60))?;
    Ok(Reading {
        windows: parsed.windows,
        performance: parsed.performance,
        details: Some(json!({"health": "online", "spend_period": parsed.spend_period, "texts": parsed.texts})),
        ..Default::default()
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Detection {
    pub result: String,
    pub preset: Option<String>,
}

/// Which usage preset fits this address. Priority order as the Mac. A 401 anywhere means "needs a key"
/// and a transport failure means "unavailable", so neither is misreported as unsupported.
pub fn detect(e: &CustomEndpoint, key: Option<&str>) -> Detection {
    let answer = |result: &str, preset: Option<&str>| Detection {
        result: result.into(),
        preset: preset.map(str::to_owned),
    };
    if net::parse_base(&e.base_url).is_err() {
        return answer("unsupported", None);
    }
    let key = key.filter(|k| !k.trim().is_empty());
    let headers = key.map_or(vec![], |k| auth_headers(&e.api_type, &e.header_key, k));
    let probes: Vec<(&str, String)> = ["abacus", "openRouter", "vllm", "newAPI", "litellm"]
        .into_iter()
        .filter_map(|p| Some((p, preset_url(p, &e.base_url)?)))
        .filter(|(_, url)| key.is_none() || net::key_allowed(url))
        .collect();
    if probes.is_empty() {
        return answer("unsupported", None);
    }
    let results: Vec<(&str, Result<Reply, net::NetError>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = probes
            .iter()
            .map(|(preset, url)| {
                let headers = &headers;
                scope.spawn(move || (*preset, net::get(url, headers, net::ENDPOINT_TIMEOUT)))
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    for (preset, result) in &results {
        let Ok(reply) = result else { continue };
        if !(200..300).contains(&reply.status) {
            continue;
        }
        if *preset == "vllm" {
            if parse_preset("vllm", &reply.body).is_some() {
                return answer("matched", Some("vllm"));
            }
            if parse_preset("llamaCpp", &reply.body).is_some() {
                return answer("matched", Some("llamaCpp"));
            }
        } else if parse_preset(preset, &reply.body).is_some() {
            return answer("matched", Some(preset));
        }
    }
    let status = |f: &dyn Fn(&Reply) -> bool| results.iter().any(|(_, r)| r.as_ref().is_ok_and(f));
    if status(&|r| matches!(r.status, 401 | 403)) {
        return answer("needsAuth", None);
    }
    let down = results.iter().any(|(_, r)| r.is_err()) || status(&|r| !(200..300).contains(&r.status) && r.status != 404);
    answer(if down { "unavailable" } else { "unsupported" }, None)
}

#[derive(Debug, Clone, Serialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub header_key: String,
    pub default_model: String,
    pub icon_preset: String,
    pub accent_color_hex: String,
}

pub fn presets() -> Vec<Preset> {
    [
        ("openrouter", "OpenRouter", "https://openrouter.ai/api/v1", "openai/gpt-4o", "openai", "#6366F1"),
        ("groq", "Groq", "https://api.groq.com/openai/v1", "llama-3.3-70b-versatile", "grok", "#F97316"),
        ("together", "Together AI", "https://api.together.xyz/v1", "meta-llama/Llama-3.3-70B-Instruct-Turbo", "meta", "#06B6D4"),
        ("mistral", "Mistral AI", "https://api.mistral.ai/v1", "mistral-large-latest", "mistral", "#F59E0B"),
        ("deepinfra", "DeepInfra", "https://api.deepinfra.com/v1/openai", "meta-llama/Meta-Llama-3.1-70B-Instruct", "deepseek", "#3B82F6"),
        ("vllm", "Local vLLM", "http://localhost:8000/v1", "", "ollama", "#10B981"),
        ("llamacpp", "Local llama.cpp", "http://localhost:8080/v1", "", "llamacpp", "#8B5CF6"),
        ("ollamaproxy", "Local Ollama Proxy", "http://localhost:11434/v1", "", "ollama-local", "#14B8A6"),
    ]
    .into_iter()
    .map(|(id, name, base, model, icon, accent)| Preset {
        id: id.into(),
        name: name.into(),
        base_url: base.into(),
        header_key: "Authorization".into(),
        default_model: model.into(),
        icon_preset: icon.into(),
        accent_color_hex: accent.into(),
    })
    .collect()
}

/// An explicit Settings action: which common local servers answer a model listing right now.
pub fn scan_local() -> Vec<Preset> {
    let candidates = [
        ("Local vLLM", 8000, "ollama", "#10B981"),
        ("Local llama.cpp", 8080, "llamacpp", "#8B5CF6"),
        ("Local LM Studio / Proxy", 1234, "lmstudio", "#8B5CF6"),
        ("Local Ollama", 11434, "ollama-local", "#14B8A6"),
        ("Local AI Server", 5000, "openai", "#3B82F6"),
    ];
    let mut found: Vec<Preset> = std::thread::scope(|scope| {
        let handles: Vec<_> = candidates
            .iter()
            .map(|(name, port, icon, accent)| {
                scope.spawn(move || {
                    let base = format!("http://127.0.0.1:{port}/v1");
                    let reply = net::get(&format!("{base}/models"), &[], Duration::from_millis(1200)).ok()?;
                    (200..300).contains(&reply.status).then(|| Preset {
                        id: format!("detected-{port}"),
                        name: format!("{name} (:{port})"),
                        base_url: base,
                        header_key: "Authorization".into(),
                        default_model: String::new(),
                        icon_preset: (*icon).into(),
                        accent_color_hex: (*accent).into(),
                    })
                })
            })
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

/// Updates the rolling history with today's total, the last 31 days at most.
pub fn record_history(e: &mut CustomEndpoint, tokens_m: f64, day: &str) {
    let total = (tokens_m * 1_000_000.0).round().max(0.0) as i64;
    e.usage_history.retain(|d| d.day != day);
    e.usage_history.push(UsageDay {
        day: day.to_string(),
        total_tokens: total,
    });
    e.usage_history.sort_by(|a, b| a.day.cmp(&b.day));
    if e.usage_history.len() > 31 {
        let extra = e.usage_history.len() - 31;
        e.usage_history.drain(..extra);
    }
    e.current_tokens_used_m = Some(tokens_m);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers_plus::net::testkit;

    fn draft(base: &str) -> CustomEndpoint {
        serde_json::from_value(json!({"name":"E","base_url":base})).unwrap()
    }

    #[test]
    fn a_key_in_json_never_reaches_the_record() {
        let e: CustomEndpoint = serde_json::from_value(json!({"name":"E","base_url":"https://x.example","apiKey":"secret","api_key":"secret"})).unwrap();
        assert!(!serde_json::to_string(&e).unwrap().contains("secret"));
    }

    #[test]
    fn validation_covers_address_header_and_origin() {
        assert!(validate(draft("https://x.example/v1")).is_ok());
        assert!(validate(draft("ftp://x.example")).is_err());
        assert!(validate(draft("https://u:p@x.example")).is_err());
        assert!(validate(draft("https://x.example?y=1")).is_err());
        let mut e = draft("https://x.example");
        e.header_key = "Host".into();
        assert!(validate(e).is_err());
        let mut e = draft("https://x.example");
        e.usage_url = Some("https://other.example/usage".into());
        assert!(validate(e).unwrap_err().contains("match"));
        let mut e = draft("https://x.example");
        e.usage_url = Some("https://x.example:443/usage".into());
        assert!(validate(e).is_ok());
        let mut e = draft("https://x.example");
        e.monthly_budget_usd = Some(-1.0);
        assert!(validate(e).is_err());
        let mut e = draft("https://x.example");
        e.has_key = true;
        assert!(!validate(e).unwrap().has_key);
    }

    #[test]
    fn protocols_build_their_own_model_urls_and_auth() {
        assert_eq!(models_url("https://a.example/v1", "openAICompatible").unwrap(), "https://a.example/v1/models");
        assert_eq!(models_url("https://a.example", "anthropic").unwrap(), "https://a.example/v1/models");
        assert_eq!(models_url("https://a.example/v1/", "anthropic").unwrap(), "https://a.example/v1/models");
        assert_eq!(models_url("https://a.example", "google").unwrap(), "https://a.example/v1beta/models");
        assert_eq!(models_url("https://a.example/v1beta", "google").unwrap(), "https://a.example/v1beta/models");
        assert_eq!(models_url("https://a.example/custom/models", "openAICompatible").unwrap(), "https://a.example/custom/models");
        assert_eq!(auth_headers("openAICompatible", "Authorization", "k")[0].1, "Bearer k");
        assert_eq!(auth_headers("openAICompatible", "Authorization", "Bearer k")[0].1, "Bearer k");
        assert_eq!(auth_headers("openAICompatible", "x-key", "k")[0], ("x-key".to_string(), "k".to_string()));
        assert_eq!(auth_headers("anthropic", "ignored", "k")[0].0, "x-api-key");
        assert_eq!(auth_headers("anthropic", "ignored", "k")[1].1, "2023-06-01");
        assert_eq!(auth_headers("google", "ignored", "k")[0].0, "x-goog-api-key");
        assert!(auth_headers("google", "ignored", "bad\nkey").is_empty());
    }

    #[test]
    fn model_discovery_per_protocol() {
        assert_eq!(parse_models(&json!({"data":[{"id":"b"},{"id":"a"},{"id":"a"}]}), "openAICompatible"), ["a", "b"]);
        assert_eq!(parse_models(&json!({"models":[{"name":"n"},{"id":"i"}]}), "openAICompatible"), ["i", "n"]);
        assert_eq!(parse_models(&json!({"data":[{"id":"claude"}]}), "anthropic"), ["claude"]);
        assert_eq!(parse_models(&json!({"models":[{"name":"models/gemini"}]}), "google"), ["gemini"]);
        let many: Vec<Value> = (0..500).map(|i| json!({"id": format!("m{i:03}")})).collect();
        assert_eq!(parse_models(&json!({"data":many}), "openAICompatible").len(), MAX_MODELS);
    }

    #[test]
    fn test_endpoint_reports_categories_only() {
        let server = testkit::serve(vec![
            ("/good/models", testkit::ok(r#"{"data":[{"id":"m"}]}"#)),
            ("/denied/models", testkit::response("401 Unauthorized", "", "private server detail")),
            ("/moved/models", testkit::response("302 Found", "Location: http://127.0.0.1:1/\r\n", "")),
        ]);
        let good = test_endpoint(&draft(&format!("{}/good", server.base)), Some("synthetic"));
        assert_ne!(good.health, "unreachable");
        assert_eq!(good.models, ["m"]);
        assert!(server.saw("Authorization: Bearer synthetic"));
        let denied = test_endpoint(&draft(&format!("{}/denied", server.base)), None);
        assert_eq!(denied.health, "unreachable");
        assert_eq!(denied.error.as_deref(), Some("The endpoint answered 401"));
        let moved = test_endpoint(&draft(&format!("{}/moved", server.base)), Some("k"));
        assert_eq!(moved.health, "unreachable");
        let public = test_endpoint(&draft("http://8.8.8.8/v1"), Some("k"));
        assert!(public.error.unwrap().contains("plain HTTP"));
    }

    #[test]
    fn preset_urls_stay_on_their_own_hosts() {
        assert_eq!(preset_url("openRouter", "https://openrouter.ai/api/v1").unwrap(), "https://openrouter.ai/api/v1/key");
        assert!(preset_url("openRouter", "https://evil.example/api/v1").is_none());
        assert!(preset_url("openRouter", "http://openrouter.ai/api/v1").is_none());
        assert!(preset_url("abacus", "https://evil.example").is_none());
        assert_eq!(
            preset_url("abacus", "https://routellm.abacus.ai/v1").unwrap(),
            "https://routellm.abacus.ai/api/v0/_getOrganizationComputePoints"
        );
        assert_eq!(preset_url("litellm", "http://h:4000/v1").unwrap(), "http://h:4000/key/info");
        assert_eq!(preset_url("newAPI", "https://h/proxy/v1/").unwrap(), "https://h/proxy/api/usage/token");
        assert_eq!(preset_url("llamaCpp", "http://h:8080/v1").unwrap(), "http://h:8080/metrics");
        assert!(preset_url("unknown", "http://h").is_none());
    }

    #[test]
    fn every_preset_format_parses_and_rejects() {
        assert_eq!(parse_preset("litellm", br#"{"info":{"spend":1.5}}"#).unwrap().spend_period, Some("lifetime"));
        assert!(parse_preset("litellm", br#"{"info":{"spend":-1}}"#).is_none());
        let or = parse_preset("openRouter", br#"{"data":{"usage_monthly":12.34}}"#).unwrap();
        assert_eq!(or.spend_period, Some("month"));
        assert_eq!(or.windows[0].count, Some(12));
        assert_eq!(or.texts["preset-spend"]["used_text"], "$12.34");
        let quota = parse_preset("newAPI", br#"{"data":{"object":"token_usage","total_used":25,"total_granted":100}}"#).unwrap();
        assert_eq!(quota.windows[0].used, 0.25);
        let unlimited = parse_preset("newAPI", br#"{"data":{"object":"token_usage","total_used":25,"total_granted":100,"unlimited_quota":true}}"#).unwrap();
        assert_eq!(unlimited.windows[0].count, Some(25));
        assert!(parse_preset("newAPI", br#"{"data":{"object":"other","total_used":1}}"#).is_none());
        assert_eq!(parse_preset("vllm", b"vllm:prompt_tokens_total 2\nvllm:generation_tokens_total 3\n").unwrap().windows[0].count, Some(5));
        let llama = parse_preset("llamaCpp", b"llamacpp:prompt_tokens_total 2\nllamacpp:tokens_predicted_total 3\n").unwrap();
        assert_eq!(llama.windows.len(), 1);
        assert_eq!(llama.performance.unwrap().total_tokens, Some(5));
        let abacus = parse_preset("abacus", br#"{"success":true,"result":{"computePointsLeft":250,"totalComputePoints":1500,"normalMonthlyCredits":1000}}"#).unwrap();
        assert_eq!(abacus.windows[0].used, 0.75);
        assert!(abacus.texts["preset-credits"]["detail"].as_str().unwrap().contains("extra"));
        assert!(parse_preset("abacus", br#"{"success":false,"result":{}}"#).is_none());
    }

    #[test]
    fn mapped_json_usage_rules() {
        let body = br#"{"a":{"rows":[{"m":{"id":"x"},"t":1000000},{"m":{"id":"y"},"t":500000}]}}"#;
        assert_eq!(parse_json_usage(body, "a.rows", "m.id", "t", None), Some(1.5));
        assert_eq!(parse_json_usage(body, "a.rows", "m.id", "t", Some("y")), Some(0.5));
        assert_eq!(parse_json_usage(body, "a.rows", "m.id", "t", Some("none")), None);
        assert_eq!(parse_json_usage(br#"[]"#, "", "m", "t", None), Some(0.0));
        assert_eq!(parse_json_usage(br#"[{"t":true}]"#, "", "m", "t", None), None);
        assert_eq!(parse_json_usage(br#"[{"t":-1}]"#, "", "m", "t", None), None);
        assert_eq!(parse_json_usage(br#"{"x":1}"#, "x", "m", "t", None), None);
        assert_eq!(parse_json_usage(br#"{"rows":[{"t":3}]}"#, "rows.0", "m", "t", None), None);
    }

    #[test]
    fn mapping_import_is_strict_and_same_origin() {
        let good = r#"{"version":1,"unit":"tokens","usageURL":"https://a.example/u","recordsPath":"","modelField":"m","tokenField":"t"}"#;
        assert_eq!(import_mapping("https://a.example/v1", good).unwrap().usage_url, "https://a.example/u");
        assert!(import_mapping("https://b.example", good).unwrap_err().contains("match"));
        assert!(import_mapping("https://a.example", &good.replace("\"version\":1", "\"version\":2")).is_err());
        assert!(import_mapping("https://a.example", &good.replace("tokens", "dollars")).is_err());
        assert!(import_mapping("https://a.example", &good.replace("\"unit\"", "\"extra\":1,\"unit\"")).unwrap_err().contains("unrecognized"));
        assert!(import_mapping("https://a.example", "{").is_err());
        assert!(import_mapping("https://a.example", &" ".repeat(MAX_MAPPING + 1)).unwrap_err().contains("64 KiB"));
    }

    #[test]
    fn no_figure_is_invented_for_manual_tracking() {
        let e = draft("https://a.example");
        assert!(tracking_windows(&e, None, 0).0.is_empty());
        let mut e = draft("https://a.example");
        e.monthly_budget_usd = Some(100.0);
        assert!(tracking_windows(&e, None, 0).0.is_empty());
        e.current_spend_usd = Some(25.0);
        let (windows, texts) = tracking_windows(&e, None, 1_790_000_000_000);
        assert_eq!(windows[0].used, 0.25);
        assert!(windows[0].resets_at.is_some());
        assert_eq!(texts["monthly-budget"]["detail"], "$25.00 / $100.00");
        e.display_remaining = true;
        let (windows, texts) = tracking_windows(&e, None, 0);
        assert_eq!(windows[0].used, 0.25);
        assert_eq!(windows[0].label, "Remaining Budget");
        assert_eq!(texts["monthly-budget"]["detail"], "$75.00 / $100.00 remaining");
        e.monthly_budget_usd = None;
        assert_eq!(tracking_windows(&e, None, 0).0[0].id, "spend-tracking");
        let mut t = draft("https://a.example");
        t.tracking_unit = "tokens".into();
        t.monthly_budget_tokens_m = Some(10.0);
        assert_eq!(tracking_windows(&t, Some(2.5), 0).0[0].used, 0.25);
        t.monthly_budget_tokens_m = None;
        assert_eq!(tracking_windows(&t, Some(2.5), 0).0[0].count, Some(2_500_000));
    }

    #[test]
    fn number_formats_match_the_mac() {
        assert_eq!(format_tokens_m(1500.0), "1.5B");
        assert_eq!(format_tokens_m(12.4), "12M");
        assert_eq!(format_tokens_m(1.0), "1M");
        assert_eq!(format_tokens_m(2.34), "2.3M");
        assert_eq!(format_tokens_m(0.0025), "2.5k");
        assert_eq!(format_tokens_m(0.0), "0M");
        assert_eq!(format_credits(934.0), "934");
        assert_eq!(format_credits(19_065.0), "19.1K");
        assert_eq!(format_credits(20_000.0), "20K");
    }

    #[test]
    fn reading_an_endpoint_end_to_end() {
        let server = testkit::serve(vec![
            ("/ok/models", testkit::ok(r#"{"data":[{"id":"m"}]}"#)),
            ("/locked/models", testkit::response("401 Unauthorized", "", "")),
            ("/rows", testkit::ok(r#"[{"m":"x","t":2000000}]"#)),
            ("/limited/key/info", testkit::response("429 Too Many Requests", "Retry-After: 300\r\n", "")),
            ("/lite/key/info", testkit::ok(r#"{"info":{"spend":3}}"#)),
        ]);
        let ok = draft(&format!("{}/ok", server.base));
        let r = read(&ok, None, 0).unwrap();
        assert!(r.windows.is_empty());
        assert!(r.note.contains("No usage"));
        assert_ne!(r.details.as_ref().unwrap()["health"], "unreachable");
        assert_eq!(read(&draft(&format!("{}/locked", server.base)), None, 0).unwrap_err().status, "needsAuth");

        let mut mapped = draft(&format!("{}/ok", server.base));
        mapped.usage_source = "jsonEndpoint".into();
        mapped.tracking_unit = "tokens".into();
        mapped.usage_url = Some(format!("{}/rows", server.base));
        mapped.usage_records_path = Some(String::new());
        mapped.usage_model_field = Some("m".into());
        mapped.usage_token_field = Some("t".into());
        let r = read(&mapped, None, 0).unwrap();
        assert_eq!(r.custom_tokens, Some(2.0));
        assert_eq!(r.windows[0].count, Some(2_000_000));

        let mut lite = draft(&format!("{}/lite", server.base));
        lite.usage_source = "jsonEndpoint".into();
        lite.usage_preset = Some("litellm".into());
        assert_eq!(read(&lite, None, 0).unwrap().windows[0].count, Some(3));
        let mut limited = draft(&format!("{}/limited", server.base));
        limited.usage_source = "jsonEndpoint".into();
        limited.usage_preset = Some("litellm".into());
        let failure = read(&limited, None, 0).unwrap_err();
        assert_eq!(failure.status, "backoff");
        assert_eq!(failure.retry_after, Some(300));
    }

    #[test]
    fn detection_classifies_without_guessing() {
        let lite = testkit::serve(vec![("/key/info", testkit::ok(r#"{"info":{"spend":1}}"#))]);
        assert_eq!(detect(&draft(&lite.base), None).preset.as_deref(), Some("litellm"));
        let metrics = testkit::serve(vec![("/metrics", testkit::ok("llamacpp:prompt_tokens_total 1\nllamacpp:tokens_predicted_total 2\n"))]);
        assert_eq!(detect(&draft(&metrics.base), None).preset.as_deref(), Some("llamaCpp"));
        let locked = testkit::serve(vec![("/key/info", testkit::response("401 Unauthorized", "", ""))]);
        assert_eq!(detect(&draft(&locked.base), None).result, "needsAuth");
        let nothing = testkit::serve(vec![]);
        assert_eq!(detect(&draft(&nothing.base), None).result, "unsupported");
        let broken = testkit::serve(vec![("/key/info", testkit::response("500 Oops", "", ""))]);
        assert_eq!(detect(&draft(&broken.base), None).result, "unavailable");
    }

    #[test]
    fn history_is_bounded_and_rewrites_today() {
        let mut e = draft("https://a.example");
        for day in 1..=40 {
            record_history(&mut e, 1.0, &format!("2026-01-{day:02}"));
        }
        assert_eq!(e.usage_history.len(), 31);
        record_history(&mut e, 2.0, "2026-01-40");
        assert_eq!(e.usage_history.last().unwrap().total_tokens, 2_000_000);
        assert_eq!(e.usage_history.len(), 31);
    }

    #[test]
    fn ids_and_templates() {
        assert!(valid_id("abc-1"));
        assert!(!valid_id("../x"));
        assert!(!valid_id(""));
        assert!(valid_id(&new_id(1_790_000_000_000)));
        assert_eq!(presets().len(), 8);
    }
}
