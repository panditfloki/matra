//! Count-only local response history. Runtime logs are read in bounded slices; only allowlisted
//! measurements and a hashed response identity survive parsing. No prompt, reply or raw log is saved.
use chrono::{Local, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const RETENTION_MS: u64 = 60 * 86_400_000;
const MAX_RECORDS: usize = 20_000;
const MAX_FILES: usize = 128;
const MAX_LINE: usize = 64 * 1024;
const READ_BUDGET: usize = 8 * 1024 * 1024;
const FILE_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Options {
    pub enabled: bool,
    pub log_directory: Option<String>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            enabled: true,
            log_directory: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Prediction {
    pub model: String,
    pub at: u64,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub draft_tokens: Option<u64>,
    pub accepted_draft_tokens: Option<u64>,
    pub tokens_per_second: Option<f64>,
    pub time_to_first_token: Option<f64>,
    pub generation_seconds: Option<f64>,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub identity: String,
}
impl Prediction {
    fn valid(&self, now: u64) -> bool {
        !self.model.is_empty()
            && self.model.len() <= 256
            && !self.model.chars().any(char::is_control)
            && self.at <= now.saturating_add(60_000)
            && self.at >= now.saturating_sub(RETENTION_MS)
            && (self.input_tokens.is_some() || self.output_tokens.is_some())
            && [
                self.input_tokens,
                self.output_tokens,
                self.reasoning_tokens,
                self.draft_tokens,
                self.accepted_draft_tokens,
            ]
            .iter()
            .flatten()
            .all(|v| *v < 1_000_000_000_000)
            && [
                self.tokens_per_second,
                self.time_to_first_token,
                self.generation_seconds,
            ]
            .iter()
            .flatten()
            .all(|v| v.is_finite() && *v > 0.0 && *v < 1e12)
    }
    fn key(&self) -> String {
        if !self.identity.is_empty() {
            return self.identity.clone();
        }
        hash(serde_json::to_vec(self).unwrap_or_default())
    }
}
fn hash(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes.as_ref())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn count(v: &Value) -> Option<u64> {
    v.as_u64().filter(|n| *n < 1_000_000_000_000)
}
fn positive(v: &Value) -> Option<f64> {
    v.as_f64()
        .filter(|v| v.is_finite() && *v > 0.0 && *v < 1e12)
}
fn local_stamp(s: &str, pattern: &str) -> Option<u64> {
    let naive = NaiveDateTime::parse_from_str(s, pattern).ok()?;
    // An ambiguous daylight-saving timestamp cannot safely be assigned to a response.
    u64::try_from(
        Local
            .from_local_datetime(&naive)
            .single()?
            .timestamp_millis(),
    )
    .ok()
}

#[derive(Default)]
struct Capture {
    value: Prediction,
    block: Option<bool>,
}
#[derive(Default)]
struct Parser {
    pending: Vec<u8>,
    dropping: bool,
    limited: bool,
    capture: Option<Capture>,
}
impl Parser {
    fn append(&mut self, provider: &str, bytes: &[u8]) -> Vec<Prediction> {
        let mut out = Vec::new();
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            let complete = part.last() == Some(&b'\n');
            if !self.dropping {
                if self.pending.len() + part.len() > MAX_LINE {
                    self.pending.clear();
                    self.dropping = true;
                    self.limited = true;
                    self.capture = None;
                } else {
                    self.pending.extend_from_slice(part);
                }
            }
            if complete {
                if !self.dropping {
                    let line = String::from_utf8_lossy(&self.pending)
                        .trim_end_matches(['\n', '\r'])
                        .to_string();
                    if provider == "lmstudio" {
                        self.lm_line(&line, &mut out);
                    } else if let Some(p) = omlx_line(&line) {
                        out.push(p);
                    }
                }
                self.pending.clear();
                self.dropping = false;
            }
        }
        out
    }
    fn lm_line(&mut self, line: &str, out: &mut Vec<Prediction>) {
        if line.starts_with('[') {
            // Never finish an interrupted response: a torn JSON block is not a completed request.
            self.capture = None;
            let Some(stamp) = line.get(1..20).filter(|_| line.get(20..21) == Some("]")) else {
                return;
            };
            let Some(at) = local_stamp(stamp, "%Y-%m-%d %H:%M:%S") else {
                return;
            };
            let Some((_, tail)) = line
                .get(21..)
                .and_then(|v| v.strip_prefix('['))
                .and_then(|v| v.split_once(']'))
            else {
                return;
            };
            let Some((model, message)) = tail.strip_prefix('[').and_then(|v| v.split_once(']'))
            else {
                return;
            };
            if message.trim_start().starts_with("Generated prediction: {") && model.len() <= 256 {
                self.capture = Some(Capture {
                    value: Prediction {
                        model: model.into(),
                        at,
                        ..Default::default()
                    },
                    block: None,
                });
            }
            return;
        }
        let Some(capture) = self.capture.as_mut() else {
            return;
        };
        if line == "}" {
            let mut value = self.capture.take().unwrap().value;
            if value.tokens_per_second.is_none() {
                value.tokens_per_second = value
                    .output_tokens
                    .zip(value.generation_seconds)
                    .filter(|(n, _)| *n > 0)
                    .map(|(n, seconds)| n as f64 / seconds);
            }
            if value.input_tokens.is_some() || value.output_tokens.is_some() {
                out.push(value);
            }
            return;
        }
        if line.starts_with("  \"id\":") || line.starts_with("  \"response_id\":") {
            if let Some(id) = scalar(line).and_then(|(_, v)| v.as_str().map(str::to_owned)) {
                capture.value.identity =
                    hash(format!("{}:{}:{id}", capture.value.model, capture.value.at));
            }
            return;
        }
        if line.starts_with("  \"usage\":") || line.starts_with("  \"stats\":") {
            let usage = line.starts_with("  \"usage\":");
            capture.block = Some(usage);
            if let Some((_, body)) = line.split_once(':') {
                if let Ok(Value::Object(fields)) =
                    serde_json::from_str(body.trim().trim_end_matches(','))
                {
                    for (key, value) in fields {
                        measurement(&mut capture.value, &key, &value);
                    }
                    capture.block = None;
                }
            }
            return;
        }
        if line.starts_with("  }") {
            capture.block = None;
            return;
        }
        if capture.block.is_some() {
            if let Some((key, value)) = scalar(line) {
                measurement(&mut capture.value, &key, &value);
            }
        }
    }
}
fn scalar(line: &str) -> Option<(String, Value)> {
    let (key, value) = line.trim().split_once(':')?;
    let key: String = serde_json::from_str(key).ok()?;
    let value: Value = serde_json::from_str(value.trim().trim_end_matches(',')).ok()?;
    Some((key, value))
}
fn measurement(p: &mut Prediction, key: &str, value: &Value) {
    match key {
        "prompt_tokens" | "input_tokens" => {
            if let Some(v) = count(value) {
                p.input_tokens = Some(v);
            }
        }
        "completion_tokens" | "total_output_tokens" => {
            if let Some(v) = count(value) {
                p.output_tokens = Some(v);
            }
        }
        "reasoning_tokens" | "reasoning_output_tokens" => p.reasoning_tokens = count(value),
        "completion_tokens_details" => {
            if let Some(v) = value.get("reasoning_tokens") {
                p.reasoning_tokens = count(v);
            }
        }
        "total_draft_tokens_count" => p.draft_tokens = count(value),
        "accepted_draft_tokens_count" => p.accepted_draft_tokens = count(value),
        "tokens_per_second" => p.tokens_per_second = positive(value),
        "time_to_first_token" | "time_to_first_token_seconds" => {
            p.time_to_first_token = positive(value)
        }
        "generation_time" => p.generation_seconds = positive(value),
        _ => (),
    }
}
fn omlx_line(line: &str) -> Option<Prediction> {
    let at = local_stamp(line.get(..23)?, "%Y-%m-%d %H:%M:%S,%3f")?;
    let tail = line.get(23..)?.strip_prefix(" - omlx.server - ")?;
    let (_, mut message) = tail.split_once(" - ")?;
    if message.starts_with('[') {
        message = message.split_once("] - ")?.1;
    }
    let prefix = [
        "Chat completion: model=",
        "Completion: model=",
        "Anthropic message: model=",
        "Responses API: model=",
    ]
    .into_iter()
    .find(|p| message.starts_with(p))?;
    let (model, rest) = message.strip_prefix(prefix)?.split_once(", ")?;
    let (output, timing) = rest.split_once(" tokens in ")?;
    let output = output
        .parse::<u64>()
        .ok()
        .filter(|v| *v > 0 && *v < 1_000_000_000_000)?;
    let (_, rate) = timing.split_once('(')?;
    let (rate, fields) = rate.split_once(" tok/s")?;
    let rate = rate
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)?;
    let fields = fields.split_once(')')?.1;
    let field = |key| fields.split(", ").find_map(|f| f.strip_prefix(key));
    Some(Prediction {
        model: model.into(),
        at,
        input_tokens: field("prompt: ").and_then(|v| v.parse().ok()),
        output_tokens: Some(output),
        tokens_per_second: Some(rate),
        generation_seconds: Some(output as f64 / rate),
        time_to_first_token: field("stream_model_ttft=")
            .and_then(|v| v.strip_suffix('s'))
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.0),
        ..Default::default()
    })
}

#[derive(Default, Clone, Serialize)]
struct Totals {
    requests: u64,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    reasoning_tokens: Option<u64>,
    draft_tokens: Option<u64>,
    accepted_draft_tokens: Option<u64>,
    input_measured: u64,
    output_measured: u64,
    reasoning_measured: u64,
    draft_measured: u64,
    accepted_draft_measured: u64,
}
impl Totals {
    fn add(&mut self, p: &Prediction) {
        self.requests += 1;
        for (sum, value) in [
            (&mut self.input_tokens, p.input_tokens),
            (&mut self.output_tokens, p.output_tokens),
            (&mut self.reasoning_tokens, p.reasoning_tokens),
            (&mut self.draft_tokens, p.draft_tokens),
            (&mut self.accepted_draft_tokens, p.accepted_draft_tokens),
        ] {
            if let Some(value) = value {
                *sum = Some(sum.unwrap_or(0).saturating_add(value));
            }
        }
        self.input_measured += u64::from(p.input_tokens.is_some());
        self.output_measured += u64::from(p.output_tokens.is_some());
        self.reasoning_measured += u64::from(p.reasoning_tokens.is_some());
        self.draft_measured += u64::from(p.draft_tokens.is_some());
        self.accepted_draft_measured += u64::from(p.accepted_draft_tokens.is_some());
    }
}
#[derive(Default)]
struct Cursor {
    fingerprint: String,
    fingerprint_len: usize,
    file_id: String,
    offset: u64,
    parser: Parser,
}
#[derive(Default)]
struct Ledger {
    scope: String,
    records: BTreeMap<String, Prediction>,
    cursors: BTreeMap<PathBuf, Cursor>,
    partial: bool,
    dirty: bool,
    cache_error: bool,
}

/// Join response measurements with the runtime's current context size for the same model. Both
/// source-poll enrichment and fresh relay projections use this; the ledger itself stores no capacity.
pub(super) fn project_context(mut history: Value, local: Option<&super::model::LocalReading>) -> Value {
    if let (Some(local), Some(models)) = (local, history["models"].as_array_mut()) {
        for model in models {
            let context = local
                .models
                .iter()
                .find(|m| Some(m.name.as_str()) == model["model"].as_str())
                .and_then(|m| m.context_length)
                .filter(|n| *n > 0);
            if let (Some(context), Some(input)) =
                (context, model["last"]["input_tokens"].as_u64())
            {
                model["context_length"] = json!(context);
                model["last_context_fraction"] = json!((input as f64 / context as f64).min(1.0));
            }
        }
    }
    history
}

pub struct Store {
    directory: PathBuf,
    options: BTreeMap<String, Options>,
    ledgers: BTreeMap<String, Ledger>,
    relay_ready: bool,
    relay_gaps: u64,
}
impl Store {
    pub fn new(directory: &Path) -> Self {
        let options = read_json(&directory.join("local-history-settings.json"), 64 * 1024)
            .and_then(|v| serde_json::from_value::<BTreeMap<String, Options>>(v).ok())
            .unwrap_or_default();
        Self {
            directory: directory.into(),
            options,
            ledgers: BTreeMap::new(),
            relay_ready: false,
            relay_gaps: 0,
        }
    }
    pub fn options(&self) -> BTreeMap<String, Options> {
        ["lmstudio", "omlx"]
            .into_iter()
            .map(|id| (id.into(), self.options.get(id).cloned().unwrap_or_default()))
            .collect()
    }
    pub fn configure(
        &mut self,
        id: &str,
        mut options: Options,
    ) -> Result<BTreeMap<String, Options>, String> {
        if !matches!(id, "lmstudio" | "omlx") {
            return Err(
                "This runtime does not publish response history in a supported server log.".into(),
            );
        }
        options.log_directory = options
            .log_directory
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty());
        if options.log_directory.as_ref().is_some_and(|v| {
            v.len() > 4096 || v.chars().any(char::is_control) || !Path::new(v).is_absolute()
        }) {
            return Err("Choose an absolute server-log directory.".into());
        }
        let mut updated = self.options.clone();
        updated.insert(id.into(), options);
        write_json(
            &self.directory.join("local-history-settings.json"),
            &updated,
        )?;
        self.options = updated;
        self.ledgers.remove(id);
        Ok(self.options())
    }
    pub fn enrich(
        &mut self,
        id: &str,
        endpoint: &str,
        home: Option<&Path>,
        reading: &mut super::model::Reading,
        now: u64,
    ) {
        if !matches!(id, "ollama" | "lmstudio" | "omlx" | "llamacpp") {
            return;
        }
        let history = project_context(self.read(id, endpoint, home, now), reading.local_model.as_ref());
        // Speed is a measured last-response value, separately labelled from current activity.
        let details = reading.details.get_or_insert_with(|| json!({}));
        if let Some(object) = details.as_object_mut() {
            object.insert("local_history".into(), history);
        }
    }
    pub fn record_ollama(
        &mut self,
        endpoint: &str,
        records: Vec<Prediction>,
        ready: bool,
        gaps: u64,
        now: u64,
    ) {
        self.relay_ready = ready;
        self.relay_gaps = gaps;
        let scope = hash(format!("ollama-relay|{endpoint}"));
        let path = self.directory.join("local-history-ollama.json");
        let ledger = self.ledgers.entry("ollama".into()).or_default();
        if ledger.scope != scope {
            *ledger = Ledger {
                scope: scope.clone(),
                ..Default::default()
            };
            if let Some(saved) =
                read_json(&path, 16 * 1024 * 1024).filter(|v| v["scope"].as_str() == Some(&scope))
            {
                if let Ok(records) =
                    serde_json::from_value::<Vec<Prediction>>(saved["records"].clone())
                {
                    for p in records
                        .into_iter()
                        .take(MAX_RECORDS)
                        .filter(|p| p.valid(now))
                    {
                        ledger.records.insert(p.key(), p);
                    }
                }
                ledger.partial = saved["partial"].as_bool().unwrap_or(false);
            }
        }
        let old_len = ledger.records.len();
        ledger.records.retain(|_, p| p.valid(now));
        ledger.dirty |= old_len != ledger.records.len();
        for p in records.into_iter().filter(|p| p.valid(now)) {
            if let std::collections::btree_map::Entry::Vacant(entry) = ledger.records.entry(p.key())
            {
                entry.insert(p);
                ledger.dirty = true;
            }
        }
        if ledger.records.len() > MAX_RECORDS {
            let mut ordered: Vec<_> = ledger
                .records
                .iter()
                .map(|(k, p)| (p.at, k.clone()))
                .collect();
            ordered.sort();
            for (_, key) in ordered.into_iter().take(ledger.records.len() - MAX_RECORDS) {
                ledger.records.remove(&key);
            }
            ledger.partial = true;
            ledger.dirty = true;
        }
        // The relay reports a cumulative gap count. Persist the first partial transition, not
        // every subsequent scheduler tick; a failed save still keeps dirty set for its retry.
        if gaps > 0 && !ledger.partial {
            ledger.partial = true;
            ledger.dirty = true;
        }
        if ledger.dirty {
            ledger.cache_error = write_json(&path, &json!({"scope":scope,"partial":ledger.partial,"records":ledger.records.values().collect::<Vec<_>>()})).is_err();
            ledger.dirty = ledger.cache_error;
        }
    }
    pub fn ollama_summary(&mut self, endpoint: &str, now: u64) -> Value {
        self.read("ollama", endpoint, None, now)
    }
    fn read(&mut self, id: &str, endpoint: &str, home: Option<&Path>, now: u64) -> Value {
        let unavailable = |state: &str, message: &str| json!({"state":state,"message":message,"retention_days":60,"days":[],"models":[],"recent":[]});
        if id == "ollama" {
            let Some(ledger) = self
                .ledgers
                .get(id)
                .filter(|l| l.scope == hash(format!("ollama-relay|{endpoint}")))
            else {
                return unavailable("disabled", "Enable the optional Ollama relay and point your client at it to observe completed responses. Direct requests are not observed.");
            };
            let state = if !self.relay_ready {
                "disabled"
            } else if ledger.partial {
                "partial"
            } else {
                "ready"
            };
            return summary(
                ledger,
                now,
                state,
                if self.relay_ready {
                    "Completed responses observed through the optional Ollama relay. Direct traffic is not included. No prompt or reply text is stored."
                } else {
                    "The Ollama relay is off or unavailable. Counts below are previously observed responses, not live activity."
                },
                ledger.cache_error,
            );
        }
        if id == "llamacpp" {
            return unavailable("unsupported", "llama.cpp exposes server-wide cumulative metrics. Per-model responses and daily token totals are unavailable.");
        }
        let options = self.options.get(id).cloned().unwrap_or_default();
        if !options.enabled {
            return unavailable(
                "disabled",
                "Server-log history is off. Loaded-model and runtime status remain available.",
            );
        }
        if !url::Url::parse(endpoint).is_ok_and(|u| super::net::is_loopback(&u)) {
            return unavailable("remote", "Server logs live on the runtime's computer. Local files are not attributed to a remote endpoint.");
        }
        let directory = options.log_directory.map(PathBuf::from).or_else(|| {
            home.map(|h| {
                h.join(if id == "lmstudio" {
                    ".lmstudio/server-logs"
                } else {
                    ".omlx/logs"
                })
            })
        });
        let Some(directory) = directory else {
            return unavailable(
                "unavailable",
                "No local home or server-log directory is available.",
            );
        };
        let scope = hash(format!("{endpoint}|{}", directory.display()));
        let path = self.directory.join(format!("local-history-{id}.json"));
        let ledger = self.ledgers.entry(id.into()).or_default();
        if ledger.scope != scope {
            *ledger = Ledger {
                scope: scope.clone(),
                ..Default::default()
            };
            if let Some(saved) =
                read_json(&path, 16 * 1024 * 1024).filter(|v| v["scope"].as_str() == Some(&scope))
            {
                if let Ok(records) =
                    serde_json::from_value::<Vec<Prediction>>(saved["records"].clone())
                {
                    for p in records
                        .into_iter()
                        .take(MAX_RECORDS)
                        .filter(|p| p.valid(now))
                    {
                        ledger.records.insert(p.key(), p);
                    }
                }
                ledger.partial = saved["partial"].as_bool().unwrap_or(false);
            }
        }
        let mut errors = false;
        let files = match log_files(id, &directory) {
            Ok((files, limited)) => {
                ledger.partial |= limited;
                files
            }
            Err(_) => {
                errors = true;
                Vec::new()
            }
        };
        let present: HashSet<_> = files.iter().cloned().collect();
        ledger.cursors.retain(|path, _| present.contains(path));
        let mut budget = READ_BUDGET;
        let mut loading = false;
        for file in &files {
            if budget == 0 {
                loading = true;
                break;
            }
            match scan_file(
                id,
                file,
                ledger.cursors.entry(file.clone()).or_default(),
                &mut budget,
            ) {
                Ok((mut records, more, clipped)) => {
                    loading |= more;
                    ledger.partial |= clipped;
                    let closed = if id == "omlx" {
                        file.file_name().is_some_and(|name| name != "server.log")
                    } else {
                        files.first() != Some(file)
                    };
                    if closed && !more {
                        if let Some(cursor) = ledger.cursors.get_mut(file) {
                            if !cursor.parser.pending.is_empty() {
                                records.extend(cursor.parser.append(id, b"\n"));
                            }
                        }
                    }
                    for p in records.into_iter().filter(|p| p.valid(now)) {
                        if let std::collections::btree_map::Entry::Vacant(entry) =
                            ledger.records.entry(p.key())
                        {
                            entry.insert(p);
                            ledger.dirty = true;
                        }
                    }
                }
                Err(_) => errors = true,
            }
        }
        let previous_len = ledger.records.len();
        ledger.records.retain(|_, p| p.valid(now));
        ledger.dirty |= ledger.records.len() != previous_len;
        if ledger.records.len() > MAX_RECORDS {
            let mut ordered: Vec<_> = ledger
                .records
                .iter()
                .map(|(k, p)| (p.at, k.clone()))
                .collect();
            ordered.sort();
            for (_, key) in ordered.into_iter().take(ledger.records.len() - MAX_RECORDS) {
                ledger.records.remove(&key);
            }
            ledger.partial = true;
            ledger.dirty = true;
        }
        // The cache contains only typed counts, timings, names and hashes. Never serialise a parser.
        if ledger.dirty {
            let cache = json!({"scope":scope,"partial":ledger.partial,"records":ledger.records.values().collect::<Vec<_>>()});
            ledger.cache_error = write_json(&path, &cache).is_err();
            ledger.dirty = ledger.cache_error;
        }
        let state = if errors {
            "unavailable"
        } else if files.is_empty() {
            "unavailable"
        } else if loading {
            "loading"
        } else if ledger.partial {
            "partial"
        } else {
            "ready"
        };
        let message = match state {
            "unavailable" => "Server logs are missing or unreadable. Any cached counts below are older observations.",
            "loading" => "Reading server logs in bounded batches. Counts grow as history is imported.",
            "partial" => "Observed responses only. Older files or responses exceeded the bounded history window.",
            _ => "Completed responses observed in server logs. Up to 60 local calendar days; no prompt or reply text is stored.",
        };
        summary(ledger, now, state, message, ledger.cache_error)
    }
}
fn read_json(path: &Path, limit: u64) -> Option<Value> {
    let f = fs::File::open(path).ok()?;
    if f.metadata().ok()?.len() > limit {
        return None;
    }
    serde_json::from_reader(f.take(limit)).ok()
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| "Cannot encode history.")?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("History cache reached its size limit.".into());
    }
    fs::create_dir_all(path.parent().ok_or("No history directory.")?)
        .map_err(|_| "Cannot create history directory.")?;
    let pending = path.with_extension("json.pending");
    fs::write(&pending, bytes).map_err(|_| "Cannot write history.")?;
    fs::rename(&pending, path).map_err(|_| {
        let _ = fs::remove_file(&pending);
        "Cannot save history.".into()
    })
}
fn log_files(id: &str, root: &Path) -> std::io::Result<(Vec<PathBuf>, bool)> {
    let mut files = Vec::new();
    let mut limited = false;
    let mut entries: Vec<_> = fs::read_dir(root)?
        .take(1025)
        .filter_map(Result::ok)
        .collect();
    if entries.len() > 1024 {
        limited = true;
        entries.truncate(1024);
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
    for entry in entries {
        let kind = entry.file_type()?;
        // No symlinks: an unrelated profile file must not be pulled into a log directory scan.
        if id == "lmstudio" && kind.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.len() != 7
                || !name.bytes().enumerate().all(|(i, b)| {
                    if i == 4 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                })
            {
                continue;
            }
            let children: Vec<_> = fs::read_dir(entry.path())?
                .take(1025)
                .filter_map(Result::ok)
                .collect();
            if children.len() > 1024 {
                limited = true;
            }
            for child in children.into_iter().take(1024) {
                if child.file_type()?.is_file()
                    && child.path().extension().is_some_and(|e| e == "log")
                {
                    files.push(child.path());
                }
            }
        } else if id == "omlx" && kind.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "server.log"
                || (name.starts_with("server.log.")
                    && name.len() == 21
                    && name[11..].bytes().enumerate().all(|(i, b)| {
                        if i == 4 || i == 7 {
                            b == b'-'
                        } else {
                            b.is_ascii_digit()
                        }
                    }))
            {
                files.push(entry.path());
            }
        }
        if files.len() > MAX_FILES * 2 {
            limited = true;
            break;
        }
    }
    files.sort_by_cached_key(|p| {
        let name = p.file_name().unwrap_or_default().to_string_lossy();
        let key = if id == "omlx" {
            (
                if name == "server.log" {
                    "9999".into()
                } else {
                    name.to_string()
                },
                0,
            )
        } else {
            let mut parts = name.split('.');
            (
                parts.next().unwrap_or("").to_owned(),
                parts
                    .next()
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0),
            )
        };
        std::cmp::Reverse(key)
    });
    if files.len() > MAX_FILES {
        limited = true;
        files.truncate(MAX_FILES);
    }
    Ok((files, limited))
}
fn scan_file(
    id: &str,
    path: &Path,
    cursor: &mut Cursor,
    budget: &mut usize,
) -> std::io::Result<(Vec<Prediction>, bool, bool)> {
    let mut file = fs::File::open(path)?;
    let meta = file.metadata()?;
    let size = meta.len();
    #[cfg(unix)]
    let file_id = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}", meta.dev(), meta.ino())
    };
    #[cfg(not(unix))]
    let file_id = format!("{:?}", meta.created().ok());
    let probe_len = if cursor.fingerprint_len == 0 {
        size.min(512) as usize
    } else {
        cursor.fingerprint_len.min(size as usize)
    };
    let mut prefix = vec![0u8; probe_len];
    file.read_exact(&mut prefix)?;
    let fingerprint = hash(&prefix);
    let mut clipped = false;
    if cursor.offset > size
        || (!cursor.file_id.is_empty() && cursor.file_id != file_id)
        || (cursor.fingerprint_len > 0 && cursor.fingerprint != fingerprint)
    {
        *cursor = Cursor::default();
    }
    cursor.fingerprint = fingerprint;
    cursor.fingerprint_len = probe_len;
    cursor.file_id = file_id;
    if cursor.offset == 0 && size > FILE_LIMIT {
        cursor.offset = size - FILE_LIMIT;
        cursor.parser.dropping = true;
        clipped = true;
    }
    file.seek(SeekFrom::Start(cursor.offset))?;
    let amount = (size - cursor.offset).min(*budget as u64) as usize;
    let mut data = vec![0; amount];
    file.read_exact(&mut data)?;
    *budget -= amount;
    cursor.offset += amount as u64;
    let result = cursor.parser.append(id, &data);
    Ok((
        result,
        cursor.offset < size,
        clipped || cursor.parser.limited,
    ))
}
fn day(at: u64) -> String {
    Local
        .timestamp_millis_opt(at as i64)
        .single()
        .map(|v| v.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}
fn summary(ledger: &Ledger, now: u64, state: &str, message: &str, save_failed: bool) -> Value {
    let today = day(now);
    let mut totals = Totals::default();
    let mut days: BTreeMap<String, Totals> = BTreeMap::new();
    let mut models: BTreeMap<String, BTreeMap<String, Totals>> = BTreeMap::new();
    let mut last: BTreeMap<String, Prediction> = BTreeMap::new();
    for p in ledger.records.values() {
        let d = day(p.at);
        if d == today {
            totals.add(p);
        }
        days.entry(d.clone()).or_default().add(p);
        models
            .entry(p.model.clone())
            .or_default()
            .entry(d)
            .or_default()
            .add(p);
        if last.get(&p.model).is_none_or(|old| old.at <= p.at) {
            last.insert(p.model.clone(), p.clone());
        }
    }
    let mut recent: Vec<_> = ledger.records.values().cloned().collect();
    recent.sort_by_key(|p| std::cmp::Reverse(p.at));
    recent.truncate(100);
    for p in &mut recent {
        p.identity.clear();
    }
    let mut models: Vec<_> = models.into_iter().map(|(name, days)| {
        let mut last = last.remove(&name); if let Some(p) = &mut last { p.identity.clear(); }
        json!({"model":name,"today":days.get(&today),"days":days.iter().map(|(day, totals)|json!({"day":day,"totals":totals})).collect::<Vec<_>>(),"last":last})
    }).collect();
    let model_list_truncated = models.len() > 128;
    models.sort_by_key(|model| std::cmp::Reverse(model["last"]["at"].as_u64().unwrap_or(0)));
    models.truncate(128);
    json!({"state":state,"message":message,"save_failed":save_failed,"retention_days":60,"today":totals,
        "days":days.iter().map(|(day, totals)|json!({"day":day,"totals":totals})).collect::<Vec<_>>(),"models":models,"model_list_truncated":model_list_truncated,"recent":recent})
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "matra-local-history-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }
    fn lm(stamp: &str, id: &str) -> String {
        format!("[{stamp}][INFO][model-a] Generated prediction: {{\n  \"id\": \"{id}\",\n  \"choices\": [{{\"content\":\"PRIVATE_REPLY_SENTINEL\"}}],\n  \"messages\": [{{\"content\":\"PRIVATE_PROMPT_SENTINEL\"}}],\n  \"usage\": {{\n    \"prompt_tokens\": 12,\n    \"completion_tokens\": 34,\n    \"completion_tokens_details\": {{\n      \"reasoning_tokens\": 7\n    }},\n    \"total_draft_tokens_count\": 44,\n    \"accepted_draft_tokens_count\": 23\n  }},\n  \"stats\": {{\n    \"tokens_per_second\": 17.0,\n    \"time_to_first_token\": 0.5,\n    \"generation_time\": 2.0\n  }}\n}}\n")
    }
    fn om(stamp: &str, suffix: &str) -> String {
        format!("{stamp} - omlx.server - INFO - [-] - Anthropic message: model=model-b, 120 tokens in 8.0s (30.0 tok/s){suffix}\n")
    }
    #[test]
    fn lm_streaming_chunks_extract_only_measurements() {
        let text = lm("2026-11-13 12:00:00", "private-response-id");
        let mut parser = Parser::default();
        let mut found = Vec::new();
        for part in text.as_bytes().chunks(7) {
            found.extend(parser.append("lmstudio", part));
        }
        assert_eq!(found.len(), 1);
        let p = &found[0];
        assert_eq!(
            (p.input_tokens, p.output_tokens, p.reasoning_tokens),
            (Some(12), Some(34), Some(7))
        );
        assert_eq!(p.tokens_per_second, Some(17.0));
        assert_eq!(p.accepted_draft_tokens, Some(23));
        let persisted = serde_json::to_string(&found).unwrap();
        assert!(!persisted.contains("PRIVATE_"));
        assert!(!persisted.contains("private-response-id"));
        assert!(!persisted.contains("choices"));
        assert_eq!(p.identity.len(), 64);
    }
    #[test]
    fn lm_v1_inline_and_invalid_numbers() {
        let text = "[2026-11-13 12:00:00][INFO][v1] Generated prediction: {\n  \"stats\": {\"input_tokens\": 8, \"total_output_tokens\": 0, \"reasoning_output_tokens\": 0, \"time_to_first_token_seconds\": 0.3}\n}\n";
        let found = Parser::default().append("lmstudio", text.as_bytes());
        assert_eq!(found[0].output_tokens, Some(0));
        assert_eq!(found[0].time_to_first_token, Some(0.3));
        let invalid = text
            .replace("\"input_tokens\": 8", "\"input_tokens\": true")
            .replace("\"total_output_tokens\": 0", "\"total_output_tokens\": -1");
        assert!(Parser::default()
            .append("lmstudio", invalid.as_bytes())
            .is_empty());
    }
    #[test]
    fn incomplete_and_oversized_lines_do_not_become_responses() {
        let mut parser = Parser::default();
        let truncated = lm("2026-11-13 12:00:00", "a")
            .trim_end_matches("}\n")
            .to_string();
        assert!(parser.append("lmstudio", truncated.as_bytes()).is_empty());
        assert!(parser
            .append("lmstudio", b"\n[2026-11-13 12:01:00][INFO][m] unrelated\n")
            .is_empty());
        let mut huge = vec![b'x'; MAX_LINE + 1];
        huge.push(b'\n');
        assert!(parser.append("lmstudio", &huge).is_empty());
        assert!(parser.pending.is_empty());
        assert_eq!(
            parser
                .append("lmstudio", lm("2026-11-13 12:02:00", "b").as_bytes())
                .len(),
            1
        );
    }
    #[test]
    fn omlx_rate_uses_runtime_clock_and_unknown_input_stays_unknown() {
        let p = omlx_line(om("2026-11-13 12:00:00,123", "").trim()).unwrap();
        assert_eq!(p.input_tokens, None);
        assert_eq!(p.generation_seconds, Some(4.0));
        let p = omlx_line(
            om(
                "2026-11-13 12:00:00,123",
                ", prompt: 42, stream_model_ttft=0.91s",
            )
            .trim(),
        )
        .unwrap();
        assert_eq!(p.input_tokens, Some(42));
        assert_eq!(p.time_to_first_token, Some(0.91));
        assert!(omlx_line("garbage").is_none());
        assert!(omlx_line(
            om("2026-11-13 12:00:00,123", "")
                .replace("30.0 tok/s", "NaN tok/s")
                .trim()
        )
        .is_none());
    }
    #[test]
    fn persisted_dedupe_rotation_and_restart_preserve_daily_counts() {
        let root = scratch();
        let logs = root.join("home/.omlx/logs");
        fs::create_dir_all(&logs).unwrap();
        let now = super::super::model::now_ms();
        let stamp = Local
            .timestamp_millis_opt(now as i64)
            .single()
            .unwrap()
            .format("%Y-%m-%d %H:%M:%S,%3f")
            .to_string();
        let line = om(&stamp, "");
        fs::write(logs.join("server.log"), &line).unwrap();
        let mut store = Store::new(&root.join("data"));
        let first = store.read(
            "omlx",
            "http://localhost:8000",
            Some(&root.join("home")),
            now,
        );
        assert_eq!(first["today"]["requests"], 1);
        assert!(first["today"]["input_tokens"].is_null());
        fs::rename(logs.join("server.log"), logs.join("server.log.2026-11-12")).unwrap();
        fs::write(logs.join("server.log"), &line).unwrap();
        let again = store.read(
            "omlx",
            "http://localhost:8000",
            Some(&root.join("home")),
            now,
        );
        assert_eq!(again["today"]["requests"], 1);
        let mut restarted = Store::new(&root.join("data"));
        let after = restarted.read(
            "omlx",
            "http://localhost:8000",
            Some(&root.join("home")),
            now,
        );
        assert_eq!(after["today"]["output_tokens"], 120);
        assert_eq!(after["models"].as_array().unwrap().len(), 1);
        let remote = restarted.read(
            "omlx",
            "http://192.0.2.4:8000",
            Some(&root.join("home")),
            now,
        );
        assert_eq!(remote["state"], "remote");
        assert!(remote["recent"].as_array().unwrap().is_empty());
        let other = restarted.read(
            "omlx",
            "http://localhost:8001",
            Some(&root.join("empty")),
            now,
        );
        assert_eq!(other["today"]["requests"], 0);
        assert!(other["today"]["output_tokens"].is_null());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn unavailable_and_disabled_are_not_zero_token_readings() {
        let root = scratch();
        let mut store = Store::new(&root);
        for id in ["ollama", "llamacpp", "lmstudio", "omlx"] {
            let history = store.read(
                id,
                "http://localhost:1234",
                None,
                super::super::model::now_ms(),
            );
            assert!(history.get("today").is_none());
            assert_ne!(history["state"], "ready");
        }
        store
            .configure(
                "lmstudio",
                Options {
                    enabled: false,
                    log_directory: None,
                },
            )
            .unwrap();
        assert_eq!(
            store.read("lmstudio", "http://localhost:1234", None, 0)["state"],
            "disabled"
        );
        assert!(store.configure("ollama", Options::default()).is_err());
        assert!(store
            .configure(
                "lmstudio",
                Options {
                    enabled: true,
                    log_directory: Some("relative".into())
                }
            )
            .is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn per_model_days_and_retention_use_record_timestamps() {
        let now = super::super::model::now_ms();
        let mut ledger = Ledger::default();
        for (model, at, output) in [("a", now, 8), ("a", now - 86_400_000, 9), ("b", now, 10)] {
            let p = Prediction {
                model: model.into(),
                at,
                output_tokens: Some(output),
                ..Default::default()
            };
            ledger.records.insert(p.key(), p);
        }
        let s = summary(&ledger, now, "ready", "", false);
        assert_eq!(s["today"]["output_tokens"], 18);
        assert_eq!(s["today"]["requests"], 2);
        assert_eq!(s["models"].as_array().unwrap().len(), 2);
        assert_eq!(s["days"].as_array().unwrap().len(), 2);
        assert!(!Prediction {
            model: "old".into(),
            at: now - RETENTION_MS - 1,
            output_tokens: Some(1),
            ..Default::default()
        }
        .valid(now));
        assert!(!s.to_string().contains("identity"));
    }
    #[test]
    fn runtime_review_unchanged_ollama_gaps_do_not_rewrite_cache() {
        let root = scratch();
        let now = super::super::model::now_ms();
        let endpoint = "http://127.0.0.1:11434";
        let mut store = Store::new(&root);
        let prediction = Prediction {
            model: "model-a".into(),
            at: now,
            input_tokens: Some(12),
            output_tokens: Some(34),
            ..Default::default()
        };
        store.record_ollama(endpoint, vec![prediction.clone()], true, 0, now);
        store.record_ollama(endpoint, vec![], true, 1, now);
        let path = root.join("local-history-ollama.json");
        assert_eq!(read_json(&path, 1024 * 1024).unwrap()["partial"], true);
        // Give the actual cache file an old clock value: any atomic rewrite replaces it with a
        // current timestamp. This does not rely on filesystem clock precision or sleep timing.
        fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1))
            .unwrap();
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let bytes = fs::read(&path).unwrap();
        for gaps in [1, 1, 2] {
            store.record_ollama(endpoint, vec![prediction.clone()], true, gaps, now);
            assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        let next = Prediction { at: now + 1, output_tokens: Some(7), ..prediction };
        store.record_ollama(endpoint, vec![next], true, 2, now + 1);
        assert_eq!(read_json(&path, 1024 * 1024).unwrap()["records"].as_array().unwrap().len(), 2);
        assert_ne!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
        assert_eq!(store.ollama_summary(endpoint, now + 1)["state"], "partial");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn runtime_review_ollama_failed_cache_write_is_retried_with_unchanged_gaps() {
        let root = scratch();
        let directory = root.join("data");
        fs::write(&directory, "fixture blocks the cache directory").unwrap();
        let endpoint = "http://127.0.0.1:11434";
        let now = super::super::model::now_ms();
        let mut store = Store::new(&directory);
        store.record_ollama(endpoint, vec![Prediction {
            model: "model-a".into(), at: now, output_tokens: Some(9), ..Default::default()
        }], true, 1, now);
        assert_eq!(store.ollama_summary(endpoint, now)["save_failed"], true);
        fs::remove_file(&directory).unwrap();
        store.record_ollama(endpoint, vec![], true, 1, now);
        let saved = read_json(&directory.join("local-history-ollama.json"), 1024 * 1024).unwrap();
        assert_eq!(saved["partial"], true);
        assert_eq!(saved["records"].as_array().unwrap().len(), 1);
        assert_eq!(store.ollama_summary(endpoint, now)["save_failed"], false);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn budget_resume_and_replacement_do_not_skip_appends() {
        let root = scratch();
        let path = root.join("server.log");
        let a = om("2026-11-13 12:00:00,000", "");
        fs::write(&path, &a).unwrap();
        let mut cursor = Cursor::default();
        let mut budget = 20;
        let (first, more, _) = scan_file("omlx", &path, &mut cursor, &mut budget).unwrap();
        assert!(first.is_empty());
        assert!(more);
        let mut budget = READ_BUDGET;
        let (last, more, _) = scan_file("omlx", &path, &mut cursor, &mut budget).unwrap();
        assert_eq!(last.len(), 1);
        assert!(!more);
        let b = om("2026-11-13 12:00:01,000", ", prompt: 2");
        fs::write(&path, format!("{a}{b}")).unwrap();
        let (next, _, _) = scan_file("omlx", &path, &mut cursor, &mut budget).unwrap();
        assert_eq!(next.len(), 1);
        fs::write(&path, b).unwrap();
        let (replaced, _, _) = scan_file("omlx", &path, &mut cursor, &mut budget).unwrap();
        assert_eq!(replaced.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cache_failure_preserves_counts_and_closed_rotated_lines_finish() {
        let root = scratch();
        let logs = root.join("home/.omlx/logs");
        fs::create_dir_all(&logs).unwrap();
        let now = super::super::model::now_ms();
        let stamp = Local
            .timestamp_millis_opt(now as i64)
            .single()
            .unwrap()
            .format("%Y-%m-%d %H:%M:%S,%3f")
            .to_string();
        fs::write(
            logs.join("server.log.2026-11-12"),
            om(&stamp, "").trim_end(),
        )
        .unwrap();
        fs::write(
            root.join("data"),
            "fixture prevents creating the cache directory",
        )
        .unwrap();
        let mut store = Store::new(&root.join("data"));
        let history = store.read(
            "omlx",
            "http://localhost:8000",
            Some(&root.join("home")),
            now,
        );
        assert_eq!(history["save_failed"], true);
        assert_eq!(history["today"]["output_tokens"], 120);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn mac_reference_response_shapes_match_their_count_fixtures() {
        let source = include_str!("../../../macos/Tests/LMStudioServerLogTests.swift");
        for (name, input, output) in [
            ("nativeV0", 66, 300),
            ("nativeV1", 59, 24),
            ("openAI", 109, 171),
        ] {
            let start = format!("static let {name} = \"\"\"\n");
            let raw = source
                .split_once(&start)
                .unwrap()
                .1
                .split_once("\n    \"\"\"")
                .unwrap()
                .0;
            let text = raw
                .lines()
                .map(|line| line.strip_prefix("    ").unwrap_or(line))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
            let found = Parser::default().append("lmstudio", text.as_bytes());
            assert_eq!(found.len(), 1, "{name}");
            assert_eq!(found[0].input_tokens, Some(input));
            assert_eq!(found[0].output_tokens, Some(output));
            assert!(!serde_json::to_string(&found).unwrap().contains("SECRET"));
        }
    }
}
