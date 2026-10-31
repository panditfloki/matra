//! Monitors for local model runtimes: Ollama, LM Studio, oMLX and llama.cpp. Nothing is loaded, unloaded
//! or generated from here, and only each runtime's own status listing is read.
//!
//! oMLX runs on a Mac. On Windows this reader monitors the address the user configures and does not
//! imply that the runtime itself runs on this PC.
use super::metrics;
use super::model::{count_window, fraction_window, integer, now_ms, number, Failure, LocalModel, LocalReading, Performance, Reading};
use super::net;
use crate::usage::LimitWindow;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

pub const OLLAMA_CLOUD: &str = "https://ollama.com/api/usage";
pub const OLLAMA_DEFAULT: &str = "http://127.0.0.1:11434";
pub const LMSTUDIO_DEFAULT: &str = "http://127.0.0.1:1234";
pub const OMLX_DEFAULT: &str = "http://127.0.0.1:8000";
pub const LLAMACPP_DEFAULT: &str = "http://127.0.0.1:8080";
const CLOUD_INTERVAL_MS: u64 = 300_000;

fn trimmed(v: &Value) -> Option<String> {
    super::model::non_empty(v)
}

fn parse_json(body: &[u8]) -> Option<Value> {
    serde_json::from_slice(body).ok()
}

fn loaded_window(count: usize) -> LimitWindow {
    count_window("loaded", "Models loaded", count as i64)
}

// MARK: Ollama

/// `GET /api/ps`. A wrong service must be told apart from "nothing loaded", so a listing with a bad row
/// is rejected whole.
pub fn parse_ollama_ps(root: &Value) -> Option<LocalReading> {
    let rows = root["models"].as_array()?;
    let mut seen = BTreeSet::new();
    let mut models = Vec::with_capacity(rows.len());
    for row in rows {
        let name = trimmed(&row["name"])?;
        if !seen.insert(name.clone()) {
            return None;
        }
        let size = row.get("size").filter(|v| !v.is_null()).map(integer);
        let vram = row.get("size_vram").filter(|v| !v.is_null()).map(integer);
        let context = row.get("context_length").filter(|v| !v.is_null()).map(integer);
        let non_negative = |v: &Option<Option<i64>>| !matches!(v, Some(None)) && v.flatten().is_none_or(|n| n >= 0);
        if !non_negative(&size) || !non_negative(&vram) || !non_negative(&context) || context.flatten() == Some(0) {
            return None;
        }
        models.push(LocalModel {
            name,
            memory_bytes: size.flatten(),
            memory_kind: "allocation".into(),
            gpu_memory_bytes: vram.flatten(),
            context_length: context.flatten(),
            quantization: trimmed(&row["details"]["quantization_level"]),
            expires_at: super::model::rfc3339_ms(&row["expires_at"]),
            ..Default::default()
        });
    }
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Some(LocalReading {
        models,
        measures_speed: false,
    })
}

/// `usage` is a fraction of the plan's allowance, not dollars. The API exposes no billing reset, so
/// these windows carry no reset date rather than a guess.
pub fn parse_ollama_cloud(root: &Value) -> Vec<LimitWindow> {
    let mut windows = Vec::new();
    for (id, label) in [("monthly", "Monthly usage"), ("session", "Session usage"), ("weekly", "Weekly usage")] {
        let limit = &root["limits"][id];
        if let Some(usage) = number(&limit["usage"]).filter(|u| *u > 0.0) {
            windows.push(fraction_window(id, label, usage, None));
        }
        for model in limit["models"].as_array().into_iter().flatten() {
            if let (Some(name), Some(count)) = (model["name"].as_str(), model["request_count"].as_i64()) {
                if count > 0 {
                    windows.push(count_window(&format!("{id}.{name}"), name, count));
                }
            }
        }
    }
    windows
}

/// The cloud half of the Ollama card changes slowly and is rate limited; the local half is polled often.
#[derive(Default)]
pub struct OllamaCloud {
    next: u64,
    windows: Vec<LimitWindow>,
    state: String,
    attempts: u32,
    pub backoff_until: u64,
}

impl OllamaCloud {
    fn refresh(&mut self, url: &str, key: Option<&str>, now: u64) {
        let Some(key) = key else {
            *self = OllamaCloud {
                state: "noKey".into(),
                ..Default::default()
            };
            return;
        };
        if now < self.next {
            return;
        }
        self.next = now + CLOUD_INTERVAL_MS;
        match net::get(url, &[net::bearer(key)], net::CLOUD_TIMEOUT) {
            Ok(reply) if (200..300).contains(&reply.status) => {
                self.attempts = 0;
                self.backoff_until = 0;
                match parse_json(&reply.body) {
                    Some(root) => {
                        self.windows = parse_ollama_cloud(&root);
                        self.state = "ok".into();
                    }
                    None => self.state = "error".into(),
                }
            }
            Ok(reply) if matches!(reply.status, 401 | 403) => {
                self.windows.clear();
                self.state = "needsAuth".into();
            }
            Ok(reply) if reply.status == 429 => {
                let wait = net::backoff_secs(self.attempts, reply.retry_after);
                self.attempts += 1;
                self.next = now + wait * 1000;
                self.backoff_until = self.next;
                self.state = "backoff".into();
            }
            _ => self.state = "error".into(),
        }
    }
}

pub fn read_ollama(endpoint: &str, cloud_url: &str, key: Option<&str>, cloud: &mut OllamaCloud, now: u64) -> Result<Reading, Failure> {
    cloud.refresh(cloud_url, key, now);
    let local = net::get(&format!("{endpoint}/api/ps"), &[], net::LOCAL_TIMEOUT);
    let (local_state, reading) = match local {
        Ok(reply) if reply.status == 200 => match parse_json(&reply.body).as_ref().and_then(parse_ollama_ps) {
            Some(reading) => ("ok", Some(reading)),
            None => ("error", None),
        },
        Ok(_) => ("error", None),
        Err(_) => ("offline", None),
    };
    if reading.is_none() && cloud.windows.is_empty() {
        if cloud.state == "needsAuth" {
            return Err(Failure::needs_auth("Ollama refused this key. Replace it in Settings."));
        }
        return Err(Failure::transient(if local_state == "offline" {
            "Ollama server unavailable. Open Ollama and check the server address."
        } else {
            "This server did not return an Ollama model listing."
        })
        .retry_in(15));
    }
    let mut windows = cloud.windows.clone();
    if let Some(reading) = &reading {
        windows.push(loaded_window(reading.models.len()));
    }
    Ok(Reading {
        windows,
        local_model: reading,
        details: Some(json!({"cloud": cloud.state, "local": local_state})),
        note: match cloud.state.as_str() {
            "needsAuth" => "Ollama refused this key. Local models are still shown.".into(),
            "backoff" => "Ollama's usage API asked to slow down. Waiting before retrying.".into(),
            _ if local_state == "offline" => "Ollama's local server is not running. Showing cloud usage only.".into(),
            _ => String::new(),
        },
        ..Default::default()
    })
}

// MARK: LM Studio

/// `GET /api/v1/models`. One entry per *loaded instance*, named by its id. `size_bytes` is the weights on
/// disk, not a memory reading, and is labelled so. Embedding models never generate and are left out.
pub fn parse_lmstudio(root: &Value) -> Option<LocalReading> {
    // A wrong port answers 200 with `{"error": ...}`, so the envelope has to be the listing's own.
    let rows = root["models"].as_array()?;
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for row in rows.iter().filter(|r| r["type"].as_str() == Some("llm")) {
        let key = trimmed(&row["key"])?;
        for instance in row["loaded_instances"].as_array().into_iter().flatten() {
            let name = trimmed(&instance["id"])?;
            let context = integer(&instance["config"]["context_length"]).or_else(|| integer(&row["max_context_length"]));
            let size = integer(&row["size_bytes"]);
            if !seen.insert(name.clone()) || size.is_some_and(|s| s < 0) || context.is_some_and(|c| c <= 0) {
                return None;
            }
            models.push(LocalModel {
                name,
                memory_bytes: size,
                memory_kind: "modelSize".into(),
                context_length: context,
                quantization: trimmed(&row["quantization"]["name"]),
                model_key: Some(key.clone()),
                ..Default::default()
            });
        }
    }
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Some(LocalReading {
        models,
        measures_speed: false,
    })
}

pub fn lmstudio_configured(home: &Path) -> Option<String> {
    let bytes = std::fs::read(home.join(".lmstudio/.internal/http-server-config.json")).ok()?;
    let port = parse_json(&bytes)?["port"].as_u64().filter(|p| (1..=65535).contains(p))?;
    Some(format!("http://127.0.0.1:{port}"))
}

pub fn read_lmstudio(endpoint: &str, token: Option<&str>) -> Result<Reading, Failure> {
    let mut headers = vec![];
    if let Some(token) = token {
        headers.push(net::bearer(token));
    }
    let reply = net::get(&format!("{endpoint}/api/v1/models"), &headers, net::LOCAL_TIMEOUT).map_err(|_| {
        Failure::transient("LM Studio server unavailable. Start the server in LM Studio's Developer tab and check the address.")
            .retry_in(15)
    })?;
    match reply.status {
        200 => {}
        401 | 403 => {
            return Err(Failure::needs_auth(
                "LM Studio requires an API token. Create one in LM Studio's Server Settings and add it in Settings.",
            )
            .retry_in(60))
        }
        code => {
            return Err(Failure::error(format!(
                "LM Studio returned HTTP {code}. Check the server address and configuration."
            ))
            .retry_in(30))
        }
    }
    let reading = parse_json(&reply.body)
        .as_ref()
        .and_then(parse_lmstudio)
        .ok_or_else(|| Failure::error("This server did not return an LM Studio model listing.").retry_in(30))?;
    Ok(Reading {
        windows: vec![loaded_window(reading.models.len())],
        local_model: Some(reading),
        details: Some(json!({"endpoint": endpoint})),
        ..Default::default()
    })
}

// MARK: oMLX

/// `GET /v1/models/status`. A profile entry shares its source model's engine and memory, so counting it
/// would count the same weights twice. Engines that never generate are left out.
pub fn parse_omlx(root: &Value) -> Option<LocalReading> {
    let rows = root["models"].as_array()?;
    // An entry that does not say whether it is loaded is a listing this parser cannot read, and
    // "no models loaded" would be a claim about it.
    if rows.iter().any(|r| r["loaded"].as_bool().is_none()) {
        return None;
    }
    let generates = |row: &Value| {
        let engine = row["engine_type"].as_str().unwrap_or_default().to_lowercase();
        !(engine == "embeddings" || engine == "embedding" || engine == "reranker" || engine.starts_with("audio_"))
    };
    let mut seen = BTreeSet::new();
    let mut models = Vec::new();
    for row in rows.iter().filter(|r| {
        r["loaded"].as_bool() == Some(true)
            && r["is_helper"].as_bool() != Some(true)
            && r["is_hidden"].as_bool() != Some(true)
            && r["source_model_id"].is_null()
            && generates(r)
    }) {
        let name = trimmed(&row["id"])?;
        let context = integer(&row["max_context_window"]).or_else(|| integer(&row["model_context_length"]));
        if !seen.insert(name.clone()) || context.is_some_and(|c| c <= 0) {
            return None;
        }
        let memory = [integer(&row["actual_size"]), integer(&row["estimated_size"])]
            .into_iter()
            .flatten()
            .find(|n| *n > 0);
        models.push(LocalModel {
            memory_bytes: memory,
            memory_kind: "allocation".into(),
            context_length: context,
            model_key: trimmed(&row["source_repo_id"]).or_else(|| Some(name.clone())),
            name,
            ..Default::default()
        });
    }
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Some(LocalReading {
        models,
        measures_speed: false,
    })
}

/// `GET /admin/api/activity`. Loose on purpose: a newer oMLX adding fields must still be read, and only a
/// body without the model list is the wrong service.
pub fn apply_omlx_activity(root: &Value, reading: &mut LocalReading) -> Option<()> {
    let rows = root["active_models"]["models"].as_array()?;
    for row in rows {
        let id = row["id"].as_str().filter(|s| !s.is_empty())?;
        let Some(model) = reading.models.iter_mut().find(|m| m.name == id) else {
            continue;
        };
        let generating = row["generating"].as_array().map(Vec::as_slice).unwrap_or_default();
        let prefilling = row["prefilling"].as_array().map_or(0, Vec::len);
        let activities = row["activities"].as_array().map_or(0, Vec::len);
        model.phase = if prefilling > 0 {
            Some("processingPrompt".into())
        } else if !generating.is_empty() || activities > 0 {
            Some("generating".into())
        } else {
            None
        };
        model.queued = integer(&row["waiting_requests"]).or_else(|| row["waiting"].as_array().map(|w| w.len() as i64));
        model.tokens_per_second = generating.first().and_then(|g| number(&g["tokens_per_second"]));
    }
    reading.measures_speed = true;
    Some(())
}

/// oMLX's admin routes take a session cookie, not the Bearer key. It is kept in memory only, and a
/// refused login waits, because each refusal is a warning in oMLX's own log.
#[derive(Default)]
pub struct OmlxSession {
    cookie: Option<String>,
    retry_login_at: u64,
    /// The address this session belongs to. A cookie is never presented to any other.
    bound_to: String,
}

impl OmlxSession {
    fn activity(&mut self, endpoint: &str, key: Option<&str>, now: u64) -> (Option<Value>, &'static str) {
        if self.bound_to != endpoint {
            *self = OmlxSession {
                bound_to: endpoint.to_string(),
                ..Default::default()
            };
        }
        let url = format!("{endpoint}/admin/api/activity");
        let read = |cookie: &Option<String>| {
            let headers: Vec<(String, String)> = cookie.iter().map(|c| net::header("Cookie", c.clone())).collect();
            net::get(&url, &headers, net::LOCAL_TIMEOUT)
        };
        let mut reply = match read(&self.cookie) {
            Ok(reply) => reply,
            Err(_) => return (None, "unavailable"),
        };
        if reply.status == 401 {
            self.cookie = None;
            let Some(key) = key else { return (None, "needsKey") };
            if now < self.retry_login_at {
                return (None, "needsKey");
            }
            // `remember: false`: a session cookie, since this logs in again whenever it is refused.
            let body = json!({"api_key": key, "remember": false}).to_string();
            let login = net::send("POST", &format!("{endpoint}/admin/api/login"), &[], Some(&body), net::LOCAL_TIMEOUT);
            match login {
                Ok(answer) if answer.status == 200 => {
                    self.cookie = answer
                        .cookies
                        .iter()
                        .filter_map(|c| c.split(';').next())
                        .map(str::trim)
                        .find(|c| c.contains('='))
                        .map(str::to_owned);
                }
                _ => {
                    self.retry_login_at = now + 60_000;
                    return (None, "needsKey");
                }
            }
            reply = match read(&self.cookie) {
                Ok(reply) => reply,
                Err(_) => return (None, "unavailable"),
            };
        }
        match reply.status {
            200 => (parse_json(&reply.body), "ok"),
            401 | 403 => {
                self.retry_login_at = now + 60_000;
                (None, "needsKey")
            }
            _ => (None, "unavailable"),
        }
    }
}

/// oMLX's own settings, when it happens to be on this machine. Only ever used for a loopback address.
pub fn omlx_file_key(home: &Path) -> Option<String> {
    let root = parse_json(&std::fs::read(home.join(".omlx/settings.json")).ok()?)?;
    let auth = &root["auth"];
    if auth["skip_api_key_verification"].as_bool() == Some(true) {
        return None;
    }
    auth["api_key"].as_str().and_then(net::clean_secret)
}

pub fn read_omlx(endpoint: &str, key: Option<&str>, session: &mut OmlxSession, now: u64) -> Result<Reading, Failure> {
    if key.is_some() && !net::key_allowed(endpoint) {
        return Err(Failure::error(
            "An oMLX key cannot be sent over plain HTTP to a public address. Use https or a private address.",
        )
        .retry_in(60));
    }
    let headers: Vec<(String, String)> = key.iter().map(|k| net::bearer(k)).collect();
    let reply = net::get(&format!("{endpoint}/v1/models/status"), &headers, net::LOCAL_TIMEOUT)
        .map_err(|_| Failure::transient("oMLX server unavailable. Start oMLX on the Mac and check the address.").retry_in(15))?;
    match reply.status {
        200 => {}
        401 | 403 => {
            return Err(Failure::needs_auth("oMLX refused the request. Add the Mac's oMLX API key in Settings.").retry_in(60));
        }
        code => {
            return Err(Failure::error(format!("oMLX returned HTTP {code}. Check the server address and configuration.")).retry_in(30));
        }
    }
    let mut reading = parse_json(&reply.body)
        .as_ref()
        .and_then(parse_omlx)
        .ok_or_else(|| Failure::error("This server did not return an oMLX model listing.").retry_in(30))?;
    let (activity, mut activity_state) = session.activity(endpoint, key, now);
    if let Some(activity) = activity {
        if apply_omlx_activity(&activity, &mut reading).is_none() {
            activity_state = "unavailable";
        }
    } else if activity_state == "ok" {
        activity_state = "unavailable";
    }
    let host = if url::Url::parse(endpoint).is_ok_and(|u| net::is_loopback(&u)) {
        "local"
    } else {
        "remote"
    };
    Ok(Reading {
        windows: vec![loaded_window(reading.models.len())],
        local_model: Some(reading),
        details: Some(json!({"endpoint": endpoint, "activity": activity_state, "runtime_host": host})),
        ..Default::default()
    })
}

// MARK: llama.cpp

pub fn read_llamacpp(endpoint: &str, key: Option<&str>) -> Result<Reading, Failure> {
    let headers: Vec<(String, String)> = key.iter().map(|k| net::bearer(k)).collect();
    let reply = net::get(&format!("{endpoint}/metrics"), &headers, net::LOCAL_TIMEOUT)
        .map_err(|_| Failure::transient("llama.cpp server unavailable. Start llama-server and check the address.").retry_in(15))?;
    match reply.status {
        200 => {}
        401 | 403 => return Err(Failure::needs_auth("llama-server wants an API key. Add it in Settings.").retry_in(60)),
        404 | 501 => {
            return Err(Failure::error("llama-server's metrics are off. Start it with --metrics.").retry_in(60));
        }
        code => return Err(Failure::error(format!("llama.cpp returned HTTP {code}.")).retry_in(30)),
    }
    let parsed = std::str::from_utf8(&reply.body)
        .ok()
        .and_then(metrics::llama)
        .ok_or_else(|| Failure::error("This server did not return llama.cpp metrics.").retry_in(30))?;
    let mut windows = Vec::new();
    if let Some(rate) = parsed.tokens_per_second {
        windows.push(count_window("speed", "Average generation speed (tok/s)", rate.round() as i64));
    }
    if let Some(active) = parsed.active {
        windows.push(count_window("active", "Active requests", active));
    }
    if let Some(queued) = parsed.queued {
        windows.push(count_window("queued", "Queued requests", queued));
    }
    windows.push(count_window("tokens", "Tokens since server start", parsed.total_tokens));
    Ok(Reading {
        windows,
        performance: Some(Performance {
            generation_tokens_per_second: parsed.tokens_per_second,
            active_requests: parsed.active,
            queued_requests: parsed.queued,
            total_tokens: Some(parsed.total_tokens),
        }),
        details: Some(json!({"endpoint": endpoint, "measured_at": now_ms()})),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers_plus::net::testkit;

    #[test]
    fn ollama_listing_and_a_wrong_service_are_told_apart() {
        let r = parse_ollama_ps(&json!({"models":[{"name":"b:1","size":10,"size_vram":5,"context_length":4096,
            "expires_at":"2026-10-31T10:00:00Z","details":{"quantization_level":" Q4 "}},{"name":"a:1"}]}))
        .unwrap();
        assert_eq!(r.models[0].name, "a:1");
        assert_eq!(r.models[0].memory_bytes, None);
        assert_eq!(r.models[1].gpu_memory_bytes, Some(5));
        assert_eq!(r.models[1].quantization.as_deref(), Some("Q4"));
        assert!(r.models[1].expires_at.is_some());
        assert!(parse_ollama_ps(&json!({"error":"no"})).is_none());
        assert!(parse_ollama_ps(&json!({"models":[{"name":"a"},{"name":"a"}]})).is_none());
        assert!(parse_ollama_ps(&json!({"models":[{"name":"a","size":-1}]})).is_none());
        assert!(parse_ollama_ps(&json!({"models":[{"name":" "}]})).is_none());
        assert_eq!(parse_ollama_ps(&json!({"models":[]})).unwrap().models.len(), 0);
    }

    #[test]
    fn ollama_cloud_shapes() {
        let modern = parse_ollama_cloud(&json!({"limits":{"monthly":{"usage":0.152,"models":[
            {"name":"glm","request_count":468},{"name":"zero","request_count":0}]}}}));
        assert_eq!(modern[0].id, "monthly");
        assert_eq!(modern[0].used, 0.152);
        assert_eq!(modern[0].resets_at, None);
        assert_eq!(modern[1].count, Some(468));
        assert_eq!(modern.len(), 2);
        let legacy = parse_ollama_cloud(&json!({"limits":{"session":{"usage":0.12},"weekly":{"usage":0.41}}}));
        assert_eq!(legacy.len(), 2);
        assert!(parse_ollama_cloud(&json!({"limits":{"monthly":{"usage":0}}})).is_empty());
    }

    #[test]
    fn ollama_local_and_cloud_merge_and_each_can_be_missing() {
        let server = testkit::serve(vec![
            ("/api/ps", testkit::ok(r#"{"models":[{"name":"m"}]}"#)),
            ("/usage", testkit::ok(r#"{"limits":{"monthly":{"usage":0.5}}}"#)),
        ]);
        let usage = format!("{}/usage", server.base);
        let mut cloud = OllamaCloud::default();
        let r = read_ollama(&server.base, &usage, Some("synthetic"), &mut cloud, 1000).unwrap();
        assert_eq!(r.windows[0].id, "monthly");
        assert_eq!(r.windows[1].count, Some(1));
        assert_eq!(r.details.as_ref().unwrap()["cloud"], "ok");
        // The cloud half is not asked again inside its interval.
        let before = server.requests.lock().unwrap().iter().filter(|r| r.contains("/usage")).count();
        read_ollama(&server.base, &usage, Some("synthetic"), &mut cloud, 2000).unwrap();
        let after = server.requests.lock().unwrap().iter().filter(|r| r.contains("/usage")).count();
        assert_eq!(before, after);
        // Forgetting the key forgets its reading.
        let r = read_ollama(&server.base, &usage, None, &mut cloud, 3000).unwrap();
        assert_eq!(r.windows.len(), 1);
        assert_eq!(r.details.as_ref().unwrap()["cloud"], "noKey");
    }

    #[test]
    fn ollama_offline_with_no_key_is_an_error_not_an_empty_card() {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", free.local_addr().unwrap());
        drop(free);
        let failure = read_ollama(&base, "http://127.0.0.1:1", None, &mut OllamaCloud::default(), 1).unwrap_err();
        assert_eq!(failure.status, "error");
        assert!(failure.transient);
    }

    #[test]
    fn ollama_cloud_rate_limit_keeps_local_and_sets_a_deadline() {
        let server = testkit::serve(vec![
            ("/api/ps", testkit::ok(r#"{"models":[]}"#)),
            ("/usage", testkit::response("429 Too Many Requests", "Retry-After: 600\r\n", "")),
        ]);
        let mut cloud = OllamaCloud::default();
        let r = read_ollama(&server.base, &format!("{}/usage", server.base), Some("k"), &mut cloud, 1000).unwrap();
        assert_eq!(r.details.as_ref().unwrap()["cloud"], "backoff");
        assert_eq!(cloud.backoff_until, 1000 + 600_000);
        assert!(r.note.contains("slow down"));
    }

    #[test]
    fn lmstudio_counts_instances_not_downloads() {
        let root = json!({"models":[
            {"type":"llm","key":"q/27b","quantization":{"name":"Q8"},"size_bytes":100,"max_context_length":200,
             "loaded_instances":[{"id":"q-1","config":{"context_length":50}},{"id":"q-2"}]},
            {"type":"llm","key":"unloaded","loaded_instances":[]},
            {"type":"embedding","key":"embed","loaded_instances":[{"id":"e"}]}]});
        let r = parse_lmstudio(&root).unwrap();
        assert_eq!(r.models.len(), 2);
        assert_eq!(r.models[0].context_length, Some(50));
        assert_eq!(r.models[1].context_length, Some(200));
        assert_eq!(r.models[0].memory_kind, "modelSize");
        assert_eq!(r.models[0].model_key.as_deref(), Some("q/27b"));
        assert!(parse_lmstudio(&json!({"error":"unknown path"})).is_none());
    }

    #[test]
    fn lmstudio_reads_token_rules_and_status() {
        let server = testkit::serve(vec![
            ("/good/api/v1/models", testkit::ok(r#"{"models":[]}"#)),
            ("/locked/api/v1/models", testkit::response("401 Unauthorized", "", "")),
            ("/weird/api/v1/models", testkit::ok(r#"{"error":"no"}"#)),
        ]);
        let r = read_lmstudio(&format!("{}/good", server.base), None).unwrap();
        assert_eq!(r.windows[0].count, Some(0));
        // A server that never asked for a token is not sent one.
        assert!(!server.saw("Authorization"));
        read_lmstudio(&format!("{}/good", server.base), Some("synthetic")).unwrap();
        assert!(server.saw("Authorization: Bearer synthetic"));
        assert_eq!(read_lmstudio(&format!("{}/locked", server.base), None).unwrap_err().status, "needsAuth");
        assert!(read_lmstudio(&format!("{}/weird", server.base), None).unwrap_err().note.contains("did not return"));
    }

    fn omlx_status() -> Value {
        json!({"models":[
            {"id":"Main","loaded":true,"estimated_size":10,"actual_size":9,"engine_type":"vlm","max_context_window":100},
            {"id":"profile","loaded":true,"source_model_id":"Main","engine_type":"vlm"},
            {"id":"embed","loaded":true,"engine_type":"embeddings"},
            {"id":"tts","loaded":true,"engine_type":"audio_tts"},
            {"id":"off","loaded":false},
            {"id":"helper","loaded":true,"is_helper":true}]})
    }

    #[test]
    fn omlx_listing_filters_as_the_mac_does() {
        let r = parse_omlx(&omlx_status()).unwrap();
        assert_eq!(r.models.len(), 1);
        assert_eq!(r.models[0].memory_bytes, Some(9));
        assert!(parse_omlx(&json!({"models":[{"id":"x"}]})).is_none());
        assert!(parse_omlx(&json!({"detail":"no"})).is_none());
    }

    #[test]
    fn omlx_activity_is_folded_into_its_model() {
        let mut r = parse_omlx(&omlx_status()).unwrap();
        apply_omlx_activity(
            &json!({"active_models":{"models":[{"id":"Main","waiting_requests":2,"prefilling":[],"activities":[],
                "generating":[{"tokens_per_second":33.5}]},{"id":"elsewhere"}]}}),
            &mut r,
        )
        .unwrap();
        assert_eq!(r.models[0].phase.as_deref(), Some("generating"));
        assert_eq!(r.models[0].queued, Some(2));
        assert_eq!(r.models[0].tokens_per_second, Some(33.5));
        assert!(r.measures_speed);
        assert!(apply_omlx_activity(&json!({"detail":"no"}), &mut r).is_none());
    }

    #[test]
    fn omlx_remote_monitoring_logs_in_once_and_keeps_the_key_off_the_read() {
        let status = testkit::ok(&omlx_status().to_string());
        let server = testkit::serve(vec![
            ("/v1/models/status", status),
            ("POST /admin/api/login", testkit::response("200 OK", "Set-Cookie: omlx=abc; HttpOnly\r\n", "{}")),
            ("/admin/api/activity", testkit::response("401 Unauthorized", "", "")),
        ]);
        let mut session = OmlxSession::default();
        let r = read_omlx(&server.base, Some("synthetic-key"), &mut session, 1000).unwrap();
        assert_eq!(r.local_model.as_ref().unwrap().models.len(), 1);
        assert!(server.saw("\"api_key\":\"synthetic-key\""));
        assert_eq!(r.details.as_ref().unwrap()["activity"], "needsKey");
        assert_eq!(r.details.as_ref().unwrap()["runtime_host"], "local");
        // Refused again: no second login until the wait passes.
        let logins = |s: &testkit::Server| s.requests.lock().unwrap().iter().filter(|r| r.starts_with("POST")).count();
        assert_eq!(logins(&server), 1);
        read_omlx(&server.base, Some("synthetic-key"), &mut session, 2000).unwrap();
        assert_eq!(logins(&server), 1);
    }

    #[test]
    fn omlx_cookie_is_not_presented_to_a_changed_address() {
        let first = testkit::serve(vec![
            ("/v1/models/status", testkit::ok(r#"{"models":[]}"#)),
            ("POST /admin/api/login", testkit::response("200 OK", "Set-Cookie: omlx=private; HttpOnly\r\n", "{}")),
            ("/admin/api/activity", testkit::response("401 Unauthorized", "", "")),
        ]);
        let second = testkit::serve(vec![
            ("/v1/models/status", testkit::ok(r#"{"models":[]}"#)),
            ("/admin/api/activity", testkit::ok(r#"{"active_models":{"models":[]}}"#)),
        ]);
        let mut session = OmlxSession::default();
        read_omlx(&first.base, Some("k"), &mut session, 1000).unwrap();
        assert!(first.saw("Cookie: omlx=private"));
        let r = read_omlx(&second.base, None, &mut session, 2000).unwrap();
        assert_eq!(r.details.as_ref().unwrap()["activity"], "ok");
        assert!(!second.saw("omlx=private"));
    }

    #[test]
    fn omlx_key_never_goes_to_a_public_plain_http_host() {
        let failure = read_omlx("http://8.8.8.8:8000", Some("k"), &mut OmlxSession::default(), 1).unwrap_err();
        assert!(failure.note.contains("plain HTTP"));
    }

    #[test]
    fn llamacpp_metrics_become_rows_without_invented_zeros() {
        let body = "llamacpp:prompt_tokens_total 10\nllamacpp:tokens_predicted_total 5\nllamacpp:predicted_tokens_seconds 41.6\n";
        let server = testkit::serve(vec![
            ("/a/metrics", testkit::ok(body)),
            ("/off/metrics", testkit::response("501 Not Implemented", "", "")),
            ("/html/metrics", testkit::ok("<html>")),
        ]);
        let r = read_llamacpp(&format!("{}/a", server.base), None).unwrap();
        let ids: Vec<&str> = r.windows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(ids, ["speed", "tokens"]);
        assert_eq!(r.performance.as_ref().unwrap().active_requests, None);
        assert_eq!(r.performance.as_ref().unwrap().total_tokens, Some(15));
        assert!(read_llamacpp(&format!("{}/off", server.base), None).unwrap_err().note.contains("--metrics"));
        assert!(read_llamacpp(&format!("{}/html", server.base), None).is_err());
    }

    #[test]
    fn config_files_are_read_loosely() {
        let dir = std::env::temp_dir().join(format!("matra-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".lmstudio/.internal")).unwrap();
        std::fs::create_dir_all(dir.join(".omlx")).unwrap();
        std::fs::write(dir.join(".lmstudio/.internal/http-server-config.json"), r#"{"port":4321,"x":1}"#).unwrap();
        assert_eq!(lmstudio_configured(&dir).as_deref(), Some("http://127.0.0.1:4321"));
        std::fs::write(dir.join(".omlx/settings.json"), r#"{"auth":{"api_key":"synthetic"}}"#).unwrap();
        assert_eq!(omlx_file_key(&dir).as_deref(), Some("synthetic"));
        std::fs::write(dir.join(".omlx/settings.json"), r#"{"auth":{"api_key":"s","skip_api_key_verification":true}}"#).unwrap();
        assert_eq!(omlx_file_key(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
