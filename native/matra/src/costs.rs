//! Project Activity and estimated INR costs, from the transcripts Claude Code and Codex already wrote.
//!
//! What is read, per record, and nothing else:
//!   Claude: timestamp, cwd, gitBranch, sessionId, requestId, message.id, message.model, message.usage
//!   Codex:  timestamp, session_meta id and cwd, turn_context cwd and model, token_count usage
//! Lines are parsed in memory and dropped. Prompts, answers, tool output and credentials are never kept,
//! and a returned report carries opaque project ids and short folder names, never a full path.
//!
//! Every amount is an estimate, not a bill. A turn that cannot be priced is counted as unpriced: it never
//! becomes zero, so a total with unpriced turns is a minimum and one with nothing priced is unknown.
//!
//! The USD to INR rate is the user's own editable estimate. It is not a live exchange rate.

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const PRICES_DEFAULT: &str = include_str!("../resources/prices-default.json");
const PLANS_DEFAULT: &str = include_str!("../resources/plans.json");

const DEFAULT_USD_TO_INR: f64 = 84.0;
const CURRENCY: &str = "INR";
const RATE_LABEL: &str = "Your estimate, not a live rate";
const COST_BASIS: &str = "api_list_price_estimate";
const OPENROUTER_MODELS_URL: &str = "https://openrouter.ai/api/v1/models";

// Relative weights that split a plan's price across turns. Same values as the Mac app: cached input costs
// roughly a tenth of fresh input, output several times more.
const WEIGHT_OUTPUT: f64 = 5.0;
const WEIGHT_CACHE_WRITE: f64 = 1.25;
const WEIGHT_CACHE_READ: f64 = 0.1;

const MIN_SLICE_SECONDS: i64 = 120;
const MAX_SESSIONS: usize = 500;
const MAX_CUSTOM_PRICES: usize = 500;
const MAX_ACCOUNT_SETTINGS: usize = 64;
const MAX_MODEL_CHARS: usize = 120;
const MAX_NAME_CHARS: usize = 80;
const MAX_TOKEN_COUNT: f64 = 1e15;
const MAX_RATE: f64 = 100_000.0;
const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;
const MAX_PRICES_BYTES: u64 = 16 * 1024 * 1024;
const MAX_REMOTE_BYTES: u64 = 32 * 1024 * 1024;
const GIT_ROOT_LEVELS: usize = 12;
const MTIME_SLACK_SECONDS: i64 = 86_400;

/// How far one report may read. A report that hits a limit says so through `partial`.
#[derive(Debug, Clone)]
struct Limits {
    max_files: usize,
    max_entries: usize,
    max_depth: usize,
    max_file_bytes: u64,
    max_total_bytes: u64,
    max_line_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_files: 20_000,
            max_entries: 200_000,
            max_depth: 10,
            max_file_bytes: 512 * 1024 * 1024,
            max_total_bytes: 8 * 1024 * 1024 * 1024,
            max_line_bytes: 16 * 1024 * 1024,
        }
    }
}

// ---------------------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------------------

/// USD per million tokens. A missing cache rate is unknown, not free.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelPrice {
    pub model: String,
    pub input: f64,
    pub output: f64,
    #[serde(default, alias = "cacheRead", skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    #[serde(default, alias = "cacheWrite", skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Billing {
    #[default]
    Subscription,
    Api,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AccountSettings {
    pub billing: Billing,
    /// What the owner says they pay per month. Zero is a real answer; None falls back to the plan's list price.
    pub monthly_paid_inr: Option<f64>,
    pub plan_tier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CostSettings {
    pub usd_to_inr: f64,
    pub currency: String,
    pub custom_prices: Vec<ModelPrice>,
    pub accounts: BTreeMap<String, AccountSettings>,
}

impl Default for CostSettings {
    fn default() -> Self {
        CostSettings {
            usd_to_inr: DEFAULT_USD_TO_INR,
            currency: CURRENCY.to_string(),
            custom_prices: Vec::new(),
            accounts: BTreeMap::new(),
        }
    }
}

fn valid_rate(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn valid_price(price: &ModelPrice) -> bool {
    valid_rate(price.input)
        && valid_rate(price.output)
        && price.cache_read.map_or(true, valid_rate)
        && price.cache_write.map_or(true, valid_rate)
}

fn validated(mut settings: CostSettings) -> Result<CostSettings, String> {
    if !settings.usd_to_inr.is_finite() || settings.usd_to_inr <= 0.0 || settings.usd_to_inr > MAX_RATE {
        return Err("USD to INR rate must be a positive number".to_string());
    }
    if settings.currency != CURRENCY {
        return Err("Only INR is supported".to_string());
    }
    if settings.custom_prices.len() > MAX_CUSTOM_PRICES {
        return Err("Too many custom prices".to_string());
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut prices: Vec<ModelPrice> = Vec::with_capacity(settings.custom_prices.len());
    for mut price in std::mem::take(&mut settings.custom_prices) {
        price.model = price.model.trim().to_string();
        if !valid_model_name(&price.model) {
            return Err("A custom price has an invalid model name".to_string());
        }
        if !valid_price(&price) {
            return Err("Custom prices must be zero or more".to_string());
        }
        // The same model entered twice: the later entry is the owner's latest word.
        let key = normalize_model(&price.model);
        match seen.get(&key).copied() {
            Some(at) => prices[at] = price,
            None => {
                seen.insert(key, prices.len());
                prices.push(price);
            }
        }
    }
    settings.custom_prices = prices;
    if settings.accounts.len() > MAX_ACCOUNT_SETTINGS {
        return Err("Too many account settings".to_string());
    }
    for (id, account) in settings.accounts.iter_mut() {
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err("An account setting has an invalid id".to_string());
        }
        if account.monthly_paid_inr.map_or(false, |v| !valid_rate(v)) {
            return Err("Monthly amount must be zero or more".to_string());
        }
        account.plan_tier = account
            .plan_tier
            .take()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        if account.plan_tier.as_deref().map_or(false, |t| t.len() > 128 || t.chars().any(char::is_control)) {
            return Err("An account setting has an invalid plan".to_string());
        }
    }
    Ok(settings)
}

fn read_capped(path: &Path, max: u64) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut out = Vec::new();
    file.take(max + 1).read_to_end(&mut out).ok()?;
    (out.len() as u64 <= max).then_some(out)
}

/// Writes a file so a reader sees the old bytes or the new bytes, nothing between.
/// Refuses a symlinked folder or target, which could point the write outside the app's own folder.
fn atomic_write(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    const FAILED: &str = "Could not save cost settings";
    std::fs::create_dir_all(dir).map_err(|_| FAILED.to_string())?;
    let is_link = |p: &Path| std::fs::symlink_metadata(p).map(|m| m.file_type().is_symlink()).unwrap_or(false);
    let target = dir.join(name);
    if is_link(dir) || is_link(&target) {
        return Err(FAILED.to_string());
    }
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let temporary = dir.join(format!(".{name}.{}.{nanos}.pending", std::process::id()));
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(FAILED.to_string());
    }
    // A scanner briefly holding the target on Windows is the usual reason a rename fails: try again shortly.
    let mut renamed = std::fs::rename(&temporary, &target);
    for _ in 0..3 {
        if renamed.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        renamed = std::fs::rename(&temporary, &target);
    }
    if renamed.is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(FAILED.to_string());
    }
    Ok(())
}

/// A missing, damaged or invalid file reads as the defaults; it is not rewritten until the owner saves.
fn load_settings(dir: &Path) -> CostSettings {
    read_capped(&dir.join("settings.json"), MAX_SETTINGS_BYTES)
        .and_then(|raw| serde_json::from_slice::<CostSettings>(&raw).ok())
        .and_then(|settings| validated(settings).ok())
        .unwrap_or_default()
}

fn save_settings(dir: &Path, settings: CostSettings) -> Result<CostSettings, String> {
    let settings = validated(settings)?;
    let text = serde_json::to_vec_pretty(&settings).map_err(|_| "Could not save cost settings".to_string())?;
    atomic_write(dir, "settings.json", &text)?;
    Ok(settings)
}

// ---------------------------------------------------------------------------------------------------------
// Prices and plans
// ---------------------------------------------------------------------------------------------------------

/// Lower case, no vendor path, dots as dashes: "openai/gpt-5.1-codex" and "gpt-5-1-codex" have to meet.
fn normalize_model(model: &str) -> String {
    let bare = model.rsplit('/').find(|part| !part.is_empty()).unwrap_or(model);
    bare.to_lowercase().replace('.', "-")
}

fn valid_model_name(model: &str) -> bool {
    !model.is_empty()
        && model.chars().count() <= MAX_MODEL_CHARS
        && model.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/' | '@' | '+'))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Usage {
    input: u64,
    output: u64,
    cache_read: u64,
    cache_write: u64,
}

impl Usage {
    fn tokens(&self) -> u64 {
        self.input.saturating_add(self.output).saturating_add(self.cache_read).saturating_add(self.cache_write)
    }
    fn weight(&self) -> f64 {
        self.input as f64
            + self.output as f64 * WEIGHT_OUTPUT
            + self.cache_read as f64 * WEIGHT_CACHE_READ
            + self.cache_write as f64 * WEIGHT_CACHE_WRITE
    }
    fn merge_max(&mut self, other: &Usage) {
        self.input = self.input.max(other.input);
        self.output = self.output.max(other.output);
        self.cache_read = self.cache_read.max(other.cache_read);
        self.cache_write = self.cache_write.max(other.cache_write);
    }
    fn add(&mut self, other: &Usage) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write = self.cache_write.saturating_add(other.cache_write);
    }
}

/// Prices one turn. A price belongs to this exact model: a newer version or a larger context variant may
/// have different rates and stays unpriced until someone says otherwise.
#[derive(Debug, Clone, Default)]
struct PriceTable {
    prices: HashMap<String, ModelPrice>,
}

impl PriceTable {
    fn add(&mut self, list: impl IntoIterator<Item = ModelPrice>) {
        for price in list {
            if valid_model_name(&price.model) && valid_price(&price) {
                self.prices.insert(normalize_model(&price.model), price);
            }
        }
    }

    /// Rupees at list price. Zero tokens cost nothing whatever the model; tokens with no rate are unknown.
    fn inr(&self, model: &str, usage: &Usage, rate: f64) -> Option<f64> {
        if usage.tokens() == 0 {
            return Some(0.0);
        }
        if !(rate.is_finite() && rate > 0.0) {
            return None;
        }
        let price = self.prices.get(&normalize_model(model))?;
        let mut usd = usage.input as f64 * price.input + usage.output as f64 * price.output;
        if usage.cache_read != 0 {
            usd += usage.cache_read as f64 * price.cache_read?;
        }
        if usage.cache_write != 0 {
            usd += usage.cache_write as f64 * price.cache_write?;
        }
        Some(usd / 1_000_000.0 * rate)
    }
}

#[derive(Deserialize)]
struct PriceFile {
    #[serde(default)]
    prices: Vec<Value>,
}

/// One bad entry drops that entry, not the whole list.
fn prices_from_json(raw: &[u8]) -> Vec<ModelPrice> {
    serde_json::from_slice::<PriceFile>(raw)
        .map(|file| {
            file.prices
                .into_iter()
                .filter_map(|entry| serde_json::from_value::<ModelPrice>(entry).ok())
                .filter(|p| valid_model_name(&p.model) && valid_price(p))
                .collect()
        })
        .unwrap_or_default()
}

/// Bundled list, then a refreshed list if one was fetched, then the owner's own prices on top.
fn price_table(dir: &Path, settings: &CostSettings) -> PriceTable {
    let mut table = PriceTable::default();
    table.add(prices_from_json(PRICES_DEFAULT.as_bytes()));
    if let Some(raw) = read_capped(&dir.join("prices.json"), MAX_PRICES_BYTES) {
        table.add(prices_from_json(&raw));
    }
    table.add(settings.custom_prices.iter().cloned());
    table
}

/// OpenRouter lists per-token prices as strings; keep the vendor models the CLIs run, as USD per million,
/// spelled the way the CLIs spell them. An entry without a real prompt and completion price is dropped, and
/// a missing cache rate stays missing rather than reading as free.
fn parse_openrouter(raw: &[u8]) -> Vec<ModelPrice> {
    let Ok(root) = serde_json::from_slice::<Value>(raw) else {
        return Vec::new();
    };
    let Some(list) = root.get("data").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in list {
        let Some(id) = entry.get("id").and_then(Value::as_str) else { continue };
        if id.contains(':') || !(id.starts_with("anthropic/") || id.starts_with("openai/")) {
            continue;
        }
        let Some(pricing) = entry.get("pricing") else { continue };
        let rate = |key: &str| -> Option<f64> {
            let value = pricing.get(key)?.as_str()?.parse::<f64>().ok()?;
            (value.is_finite() && value >= 0.0).then_some(value * 1e6)
        };
        let (Some(input), Some(output)) = (rate("prompt"), rate("completion")) else { continue };
        if input <= 0.0 && output <= 0.0 {
            continue;
        }
        let name = id.rsplit('/').next().unwrap_or(id).replace('.', "-");
        if !valid_model_name(&name) {
            continue;
        }
        out.push(ModelPrice {
            model: name,
            input,
            output,
            cache_read: rate("input_cache_read"),
            cache_write: rate("input_cache_write"),
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanInfo {
    pub tier: String,
    pub name: String,
    pub usd: Option<f64>,
    pub inr: Option<f64>,
    pub credit_usd: Option<f64>,
}

fn plans_from_json(raw: &[u8]) -> Option<BTreeMap<String, PlanInfo>> {
    fn money(value: Option<&Value>) -> Option<f64> {
        value.and_then(Value::as_f64).filter(|v| valid_rate(*v))
    }
    let root: Value = serde_json::from_slice(raw).ok()?;
    let list = root.get("plans")?.as_object()?;
    let mut out = BTreeMap::new();
    for (tier, plan) in list {
        let prices = plan.get("prices");
        out.insert(
            tier.clone(),
            PlanInfo {
                tier: tier.clone(),
                name: plan.get("name").and_then(Value::as_str).unwrap_or(tier).to_string(),
                usd: money(prices.and_then(|p| p.get("USD"))),
                inr: money(prices.and_then(|p| p.get(CURRENCY))),
                credit_usd: money(plan.get("credit_usd")),
            },
        );
    }
    Some(out)
}

/// The owner's edited catalog if it reads, otherwise the bundled one.
fn load_plans(dir: &Path) -> BTreeMap<String, PlanInfo> {
    read_capped(&dir.join("plans.json"), MAX_SETTINGS_BYTES)
        .and_then(|raw| plans_from_json(&raw))
        .or_else(|| plans_from_json(PLANS_DEFAULT.as_bytes()))
        .unwrap_or_default()
}

/// Monthly list price in rupees: the catalog's rupee price when it has one, else its USD price at the rate.
/// None when neither is possible. A free plan is a real zero, not a missing price.
fn plan_monthly_inr(plan: &PlanInfo, rate: f64) -> Option<f64> {
    if let Some(inr) = plan.inr {
        return Some(inr);
    }
    let usd = plan.usd?;
    if usd == 0.0 {
        return Some(0.0);
    }
    (rate > 0.0).then_some(usd * rate)
}

// ---------------------------------------------------------------------------------------------------------
// Reading transcripts
// ---------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Claude,
    Codex,
}

/// One deduplicated turn: numbers and metadata only.
#[derive(Debug, Clone)]
struct Turn {
    ts: i64,
    key: String,
    session: Arc<str>,
    /// Internal only. The report shows an opaque id and a short name instead.
    project: Arc<str>,
    branch: Option<Arc<str>>,
    model: Arc<str>,
    usage: Usage,
}

#[derive(Debug, Default)]
struct FileParse {
    turns: Vec<Turn>,
    /// Lines that were broken or missing a field every turn must have.
    malformed: u64,
    /// Lines longer than the read limit.
    long_lines: u64,
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// A token count from a transcript: a non-negative number, whole or not.
fn count(value: &Value) -> Option<u64> {
    if let Some(n) = value.as_u64() {
        return (n as f64 <= MAX_TOKEN_COUNT).then_some(n);
    }
    let n = value.as_f64()?;
    (n.is_finite() && n >= 0.0 && n <= MAX_TOKEN_COUNT).then_some(n as u64)
}

/// Absent counts as zero; present but not a count is a broken record.
fn optional_count(object: &Value, keys: &[&str]) -> Option<u64> {
    for key in keys {
        match object.get(*key) {
            None | Some(Value::Null) => continue,
            Some(value) => return count(value),
        }
    }
    Some(0)
}

fn parse_stamp(value: Option<&Value>) -> Option<DateTime<chrono::FixedOffset>> {
    DateTime::parse_from_rfc3339(value?.as_str()?).ok()
}

/// A usable model name, or the plainly unpriceable "unknown". Anything else in that field is not a model.
fn clean_model(model: &str) -> Arc<str> {
    if valid_model_name(model) {
        Arc::from(model)
    } else {
        Arc::from("unknown")
    }
}

/// Reuses the previous allocation when, as nearly always, a value repeats line after line.
fn intern(slot: &mut Option<Arc<str>>, value: &str) -> Arc<str> {
    if slot.as_deref() != Some(value) {
        *slot = Some(Arc::from(value));
    }
    slot.clone().unwrap_or_else(|| Arc::from(value))
}

/// Finds where sessions belong. Sessions started in subfolders of one repo are one project, so a cwd walks
/// up to its git root; if that is gone, the transcript folder's name recovers where the session started.
#[derive(Default)]
struct ProjectResolver {
    cache: HashMap<(String, String), Arc<str>>,
}

impl ProjectResolver {
    fn resolve(&mut self, cwd: &str, folder: &str) -> Arc<str> {
        let key = (folder.to_string(), cwd.to_string());
        if let Some(found) = self.cache.get(&key) {
            return found.clone();
        }
        let resolved: Arc<str> = Arc::from(Self::root_of(cwd, folder).as_str());
        self.cache.insert(key, resolved.clone());
        resolved
    }

    fn root_of(cwd: &str, folder: &str) -> String {
        if cwd.is_empty() {
            return String::new();
        }
        // Touching a network path named in a log can stall the app or send credentials to that server.
        let network = cwd.starts_with("\\\\") || cwd.starts_with("//");
        if !network {
            let mut dir = Path::new(cwd);
            for _ in 0..GIT_ROOT_LEVELS {
                if dir.join(".git").exists() {
                    return dir.to_string_lossy().into_owned();
                }
                match dir.parent() {
                    Some(parent) if parent != dir && parent.parent().is_some() => dir = parent,
                    _ => break,
                }
            }
        }
        // Claude Code names the folder by replacing every non-alphanumeric character with "-". The same
        // transform of the cwd finds the real prefix without decoding anything.
        if !folder.is_empty() && folder.chars().count() < cwd.chars().count() {
            let transformed: String = cwd.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
            if transformed.starts_with(folder) {
                return cwd.chars().take(folder.chars().count()).collect();
            }
        }
        cwd.to_string()
    }
}

/// Calls `line` with each complete line. A trailing line with no newline is still being written and is left
/// for a later read; a line past the limit is counted and skipped without holding it in memory.
fn for_each_line<R: BufRead>(
    mut reader: R,
    max_line: usize,
    long_lines: &mut u64,
    mut line: impl FnMut(&[u8]),
) -> std::io::Result<()> {
    let mut held: Vec<u8> = Vec::new();
    let mut skipping = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(());
        }
        let length = available.len();
        match available.iter().position(|b| *b == b'\n') {
            Some(at) => {
                if skipping {
                    skipping = false;
                } else if held.len() + at > max_line {
                    *long_lines += 1;
                } else if held.is_empty() {
                    line(&available[..at]);
                } else {
                    held.extend_from_slice(&available[..at]);
                    line(&held);
                }
                held.clear();
                reader.consume(at + 1);
            }
            None => {
                if !skipping {
                    if held.len() + length > max_line {
                        *long_lines += 1;
                        skipping = true;
                        held = Vec::new();
                    } else {
                        held.extend_from_slice(available);
                    }
                }
                reader.consume(length);
            }
        }
    }
}

fn trim_line(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    line.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(line)
}

enum Parsed {
    Turn(Turn),
    Skip,
    Malformed,
}

/// Remembers the last strings seen so a long transcript does not allocate them once per turn.
#[derive(Default)]
struct ClaudeContext {
    session: Option<Arc<str>>,
    model: Option<Arc<str>>,
    branch: Option<Arc<str>>,
}

fn parse_claude_line(line: &[u8], folder: &str, ctx: &mut ClaudeContext, resolver: &mut ProjectResolver) -> Parsed {
    // Most lines are user turns or attachments and never reach the JSON parser.
    if !contains(line, b"\"assistant\"") {
        return Parsed::Skip;
    }
    let Ok(record) = serde_json::from_slice::<Value>(line) else {
        return Parsed::Malformed;
    };
    if record.get("type").and_then(Value::as_str) != Some("assistant") {
        return Parsed::Skip;
    }
    let Some(message) = record.get("message") else {
        return Parsed::Malformed;
    };
    let (Some(usage), Some(model)) = (message.get("usage"), message.get("model").and_then(Value::as_str)) else {
        return Parsed::Malformed;
    };
    // Synthetic turns are local error placeholders, not billed requests.
    if model == "<synthetic>" {
        return Parsed::Skip;
    }
    let cwd = record.get("cwd").and_then(Value::as_str);
    let session = record
        .get("sessionId")
        .or_else(|| record.get("session_id"))
        .and_then(Value::as_str);
    let stamp = parse_stamp(record.get("timestamp"));
    let input = usage.get("input_tokens").and_then(count);
    let output = usage.get("output_tokens").and_then(count);
    let cache_read = optional_count(usage, &["cache_read_input_tokens"]);
    let cache_write = optional_count(usage, &["cache_creation_input_tokens"]);
    let (Some(cwd), Some(session), Some(stamp), Some(input), Some(output), Some(cache_read), Some(cache_write)) =
        (cwd, session, stamp, input, output, cache_read, cache_write)
    else {
        return Parsed::Malformed;
    };
    let ts = stamp.timestamp();
    // Claude Code appends the same turn several times while streaming, each write carrying a larger output
    // count, and a resumed session can copy earlier turns into a new file. Collapse them by request id.
    let key = match record.get("requestId").and_then(Value::as_str).filter(|id| !id.is_empty()) {
        Some(request) => format!("r:{request}"),
        None => match message.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()) {
            Some(id) => format!("m:{id}"),
            None => format!("s:{session}:{ts}:{input}:{output}:{cache_read}:{cache_write}"),
        },
    };
    let branch = record
        .get("gitBranch")
        .and_then(Value::as_str)
        .filter(|b| !b.is_empty())
        .map(|b| intern(&mut ctx.branch, b));
    if ctx.model.as_deref() != Some(model) {
        ctx.model = Some(clean_model(model));
    }
    let model = ctx.model.clone().unwrap_or_else(|| clean_model(model));
    Parsed::Turn(Turn {
        ts,
        key,
        session: intern(&mut ctx.session, session),
        project: resolver.resolve(cwd, folder),
        branch,
        model,
        usage: Usage { input, output, cache_read, cache_write },
    })
}

/// One Codex usage object as written: input here still includes its cached part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CodexCounters {
    input: u64,
    output: u64,
    cached: u64,
    cache_write: u64,
}

impl CodexCounters {
    fn read(value: &Value) -> Option<CodexCounters> {
        Some(CodexCounters {
            input: value.get("input_tokens").and_then(count)?,
            output: value.get("output_tokens").and_then(count)?,
            cached: optional_count(value, &["cached_input_tokens", "cache_read_input_tokens"])?,
            cache_write: optional_count(value, &["cache_write_input_tokens"])?,
        })
    }

    fn covers(&self, earlier: &CodexCounters) -> bool {
        self.input >= earlier.input
            && self.output >= earlier.output
            && self.cached >= earlier.cached
            && self.cache_write >= earlier.cache_write
    }

    fn since(&self, earlier: &CodexCounters) -> CodexCounters {
        CodexCounters {
            input: self.input - earlier.input,
            output: self.output - earlier.output,
            cached: self.cached - earlier.cached,
            cache_write: self.cache_write - earlier.cache_write,
        }
    }

    fn usage(&self) -> Usage {
        Usage {
            input: self.input.saturating_sub(self.cached),
            output: self.output,
            cache_read: self.cached,
            cache_write: self.cache_write,
        }
    }
}

/// Codex rollouts carry session, cwd and model on earlier lines than the token counts.
#[derive(Default)]
struct CodexContext {
    session: Option<Arc<str>>,
    cwd: Option<String>,
    project: Option<Arc<str>>,
    model: Option<Arc<str>>,
    /// turn_context names the model of the turns that follow. Once seen, a model named by any other record
    /// (a spawned helper, say) must not re-label this session's tokens.
    model_is_authoritative: bool,
    previous_total: Option<CodexCounters>,
}

/// The value of the first `"model":"…"` key on a line, without parsing the line.
fn model_on_line(line: &[u8]) -> Option<&str> {
    const MARKER: &[u8] = b"\"model\":\"";
    let at = line.windows(MARKER.len()).position(|w| w == MARKER)? + MARKER.len();
    let rest = &line[at..];
    let end = rest.iter().position(|b| *b == b'"')?;
    let name = std::str::from_utf8(&rest[..end]).ok()?;
    valid_model_name(name).then_some(name)
}

fn parse_codex_line(line: &[u8], fallback_session: &str, ctx: &mut CodexContext, resolver: &mut ProjectResolver) -> Parsed {
    if !ctx.model_is_authoritative {
        if let Some(model) = model_on_line(line) {
            ctx.model = Some(Arc::from(model));
        }
    }
    let wanted = contains(line, b"token_count") || contains(line, b"session_meta") || contains(line, b"turn_context");
    if !wanted {
        return Parsed::Skip;
    }
    let Ok(record) = serde_json::from_slice::<Value>(line) else {
        return Parsed::Malformed;
    };
    let Some(payload) = record.get("payload") else {
        return Parsed::Skip;
    };
    let outer = record.get("type").and_then(Value::as_str).unwrap_or("");
    let inner = payload.get("type").and_then(Value::as_str).unwrap_or("");

    if outer == "session_meta" {
        // A forked rollout repeats its parent's meta line later on; the first one is this file's own.
        if ctx.session.is_none() {
            ctx.session = payload
                .get("id")
                .or_else(|| payload.get("session_id"))
                .and_then(Value::as_str)
                .map(Arc::from);
        }
        if ctx.cwd.is_none() {
            if let Some(cwd) = payload.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty()) {
                ctx.cwd = Some(cwd.to_string());
                ctx.project = None;
            }
        }
        return Parsed::Skip;
    }
    if outer == "turn_context" || inner == "turn_context" {
        if let Some(cwd) = payload.get("cwd").and_then(Value::as_str).filter(|c| !c.is_empty()) {
            if ctx.cwd.as_deref() != Some(cwd) {
                ctx.cwd = Some(cwd.to_string());
                ctx.project = None;
            }
        }
        if let Some(model) = payload.get("model").and_then(Value::as_str).filter(|m| valid_model_name(m)) {
            ctx.model = Some(Arc::from(model));
            ctx.model_is_authoritative = true;
        }
        return Parsed::Skip;
    }
    if inner != "token_count" {
        return Parsed::Skip;
    }
    // Rate-limit-only ticks carry no usage.
    let Some(info) = payload.get("info").filter(|i| i.is_object()) else {
        return Parsed::Skip;
    };
    let total_raw = info.get("total_token_usage").filter(|v| v.is_object());
    let last_raw = info.get("last_token_usage").filter(|v| v.is_object());
    if total_raw.is_none() && last_raw.is_none() {
        return Parsed::Skip;
    }
    let total = total_raw.map(CodexCounters::read);
    let last = last_raw.map(CodexCounters::read);
    if matches!(total, Some(None)) || matches!(last, Some(None)) {
        return Parsed::Malformed;
    }
    let (total, last) = (total.flatten(), last.flatten());
    let Some(stamp) = parse_stamp(record.get("timestamp")) else {
        return Parsed::Malformed;
    };
    let millis = stamp.timestamp_millis();
    let session: Arc<str> = ctx.session.clone().unwrap_or_else(|| Arc::from(fallback_session));

    // total_token_usage is running, and Codex repeats a tick unchanged. So the turn's usage is how far the
    // running total moved. The first tick in a file, or one after the counter went backwards, cannot be
    // compared: a resumed file's first total includes earlier work, so last_token_usage is the turn's own.
    let (counters, key) = match total {
        Some(total) => {
            let counters = match ctx.previous_total {
                Some(previous) if total == previous => return Parsed::Skip,
                Some(previous) if total.covers(&previous) => total.since(&previous),
                _ => last.unwrap_or(total),
            };
            ctx.previous_total = Some(total);
            // No session in this key: a forked rollout copies its parent's ticks under a new session id.
            (counters, format!("c:{millis}:{}:{}:{}", total.input, total.output, total.cached))
        }
        None => {
            let last = last.unwrap_or(CodexCounters { input: 0, output: 0, cached: 0, cache_write: 0 });
            (last, format!("c:{session}:{millis}:{}:{}", last.input, last.output))
        }
    };
    if counters.input == 0 && counters.output == 0 {
        return Parsed::Skip;
    }
    let project = match ctx.project.as_ref() {
        Some(project) => project.clone(),
        None => {
            let project = resolver.resolve(ctx.cwd.as_deref().unwrap_or(""), "");
            ctx.project = Some(project.clone());
            project
        }
    };
    Parsed::Turn(Turn {
        ts: stamp.timestamp(),
        key,
        session,
        project,
        branch: None,
        // Nothing named a model yet: keep it visible and unpriced, as the Mac app does.
        model: ctx.model.clone().unwrap_or_else(|| Arc::from("codex")),
        usage: counters.usage(),
    })
}

fn parse_file(path: &Path, format: Format, folder: &str, limits: &Limits, resolver: &mut ProjectResolver) -> std::io::Result<FileParse> {
    let file = std::fs::File::open(path)?;
    let reader = BufReader::with_capacity(64 * 1024, file.take(limits.max_file_bytes));
    let fallback_session = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut claude = ClaudeContext::default();
    let mut codex = CodexContext::default();
    let mut turns = Vec::new();
    let mut malformed = 0u64;
    let mut long_lines = 0u64;
    for_each_line(reader, limits.max_line_bytes, &mut long_lines, |raw| {
        let line = trim_line(raw);
        if line.is_empty() {
            return;
        }
        let parsed = match format {
            Format::Claude => parse_claude_line(line, folder, &mut claude, resolver),
            Format::Codex => parse_codex_line(line, &fallback_session, &mut codex, resolver),
        };
        match parsed {
            Parsed::Turn(turn) => turns.push(turn),
            Parsed::Malformed => malformed += 1,
            Parsed::Skip => {}
        }
    })?;
    Ok(FileParse { turns, malformed, long_lines })
}

#[derive(Debug, Clone)]
struct Candidate {
    path: PathBuf,
    root: PathBuf,
    size: u64,
    modified: i64,
}

fn modified_seconds(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The `.jsonl` files under the roots that could hold turns since `since`, newest first, so a limit drops the
/// oldest. Symlinks and hidden entries are not followed. Returns the files and how many a limit dropped.
fn collect_files(roots: &[PathBuf], since: i64, limits: &Limits) -> (Vec<Candidate>, u64) {
    let mut files: Vec<Candidate> = Vec::new();
    let mut dropped = 0u64;
    let mut entries = 0usize;
    for root in roots {
        let mut stack = vec![(root.clone(), 0usize)];
        while let Some((dir, depth)) = stack.pop() {
            let Ok(listing) = std::fs::read_dir(&dir) else { continue };
            for entry in listing.flatten() {
                entries += 1;
                if entries > limits.max_entries {
                    let kept = sorted_and_capped(files, limits, &mut dropped);
                    return (kept, dropped + 1);
                }
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') {
                    continue;
                }
                let Ok(kind) = entry.file_type() else { continue };
                if kind.is_dir() {
                    if depth < limits.max_depth {
                        stack.push((entry.path(), depth + 1));
                    } else {
                        dropped += 1;
                    }
                } else if kind.is_file() && name.ends_with(".jsonl") {
                    let Ok(meta) = entry.metadata() else { continue };
                    let modified = modified_seconds(&meta);
                    if modified < since {
                        continue;
                    }
                    if meta.len() > limits.max_file_bytes {
                        dropped += 1;
                        continue;
                    }
                    files.push(Candidate { path: entry.path(), root: root.clone(), size: meta.len(), modified });
                }
            }
        }
    }
    let kept = sorted_and_capped(files, limits, &mut dropped);
    (kept, dropped)
}

fn sorted_and_capped(mut files: Vec<Candidate>, limits: &Limits, dropped: &mut u64) -> Vec<Candidate> {
    files.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.path.cmp(&b.path)));
    if files.len() > limits.max_files {
        *dropped += (files.len() - limits.max_files) as u64;
        files.truncate(limits.max_files);
    }
    files
}

struct CachedFile {
    size: u64,
    modified: i64,
    parse: Arc<FileParse>,
}

#[derive(Default)]
struct EngineState {
    files: HashMap<PathBuf, CachedFile>,
    resolver: ProjectResolver,
}

/// Holds parsed turns between reports so reopening the window only re-reads files that changed.
/// In memory only: numbers and metadata, gone when the app closes.
#[derive(Default)]
struct Engine {
    state: Mutex<EngineState>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ScanStats {
    pub files_read: u64,
    pub files_skipped: u64,
    pub lines_skipped: u64,
}

impl Engine {
    /// One account's turns from files written since `since`, deduplicated. Skipped lines only count for
    /// files that could hold turns from `report_from`, so one broken old line does not mark every later
    /// report partial.
    fn account_turns(
        &self,
        source: &AccountSource,
        since: i64,
        report_from: i64,
        limits: &Limits,
        stats: &mut ScanStats,
    ) -> Vec<Turn> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let EngineState { files: cache, resolver } = &mut *state;
        let (candidates, dropped) = collect_files(&source.roots, since, limits);
        stats.files_skipped += dropped;
        if cache.len() > limits.max_files.saturating_mul(2) {
            cache.clear();
        }

        let mut budget = limits.max_total_bytes;
        let mut by_key: HashMap<String, usize> = HashMap::new();
        let mut turns: Vec<Turn> = Vec::new();
        // Oldest first, so when two files share a turn the original's project and model are the ones kept.
        for candidate in candidates.iter().rev() {
            let cached = cache
                .get(&candidate.path)
                .filter(|c| c.size == candidate.size && c.modified == candidate.modified)
                .map(|c| c.parse.clone());
            let parse = match cached {
                Some(parse) => parse,
                None => {
                    if candidate.size > budget {
                        stats.files_skipped += 1;
                        continue;
                    }
                    budget -= candidate.size;
                    let folder = if source.format == Format::Claude { project_folder(candidate) } else { String::new() };
                    let Ok(parsed) = parse_file(&candidate.path, source.format, &folder, limits, resolver) else {
                        stats.files_skipped += 1;
                        continue;
                    };
                    let parse = Arc::new(parsed);
                    cache.insert(
                        candidate.path.clone(),
                        CachedFile { size: candidate.size, modified: candidate.modified, parse: parse.clone() },
                    );
                    parse
                }
            };
            stats.files_read += 1;
            if candidate.modified >= report_from {
                stats.lines_skipped += parse.malformed + parse.long_lines;
            }
            for turn in &parse.turns {
                match by_key.get(&turn.key).copied() {
                    Some(at) => {
                        let kept = &mut turns[at];
                        kept.ts = kept.ts.max(turn.ts);
                        kept.usage.merge_max(&turn.usage);
                        if kept.branch.is_none() {
                            kept.branch = turn.branch.clone();
                        }
                    }
                    None => {
                        by_key.insert(turn.key.clone(), turns.len());
                        turns.push(turn.clone());
                    }
                }
            }
        }
        turns
    }
}

/// The top folder under Claude's `projects`, which names where the session started. A sub-agent's transcript
/// sits deeper in the same folder, so the file's direct parent would not do.
fn project_folder(candidate: &Candidate) -> String {
    let Ok(relative) = candidate.path.strip_prefix(&candidate.root) else {
        return String::new();
    };
    let mut parts = relative.components();
    match (parts.next(), parts.next()) {
        (Some(first), Some(_)) => first.as_os_str().to_string_lossy().into_owned(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------------------------------------
// Projects
// ---------------------------------------------------------------------------------------------------------

/// Folder names that say nothing on their own, so the parent is kept too: "nodes/src", not "src".
const GENERIC_FOLDERS: &[&str] = &[
    "src", "source", "sources", "lib", "libs", "app", "apps", "core", "common", "shared", "packages", "package",
    "modules", "components", "dist", "build", "out", "bin", "scripts", "assets", "public", "static", "docs", "doc",
    "test", "tests", "spec", "api", "server", "client", "web", "windows", "macos", "ios", "android", "main", "index",
    "utils", "util",
];

/// Same project whichever way its path was spelled: slashes unified and, on a drive path, case folded.
fn normalized_path(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let trimmed = unified.trim_end_matches('/');
    let trimmed = if trimmed.is_empty() { unified.as_str() } else { trimmed };
    if trimmed.as_bytes().get(1) == Some(&b':') {
        trimmed.to_lowercase()
    } else {
        trimmed.to_string()
    }
}

const UNKNOWN_PROJECT_ID: &str = "unknown";

fn project_id(root: &str) -> String {
    if root.is_empty() {
        return UNKNOWN_PROJECT_ID.to_string();
    }
    let digest = Sha256::digest(normalized_path(root).as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

/// A short name that never carries the full path, nor the home folder's name, which is usually the username.
fn project_name(root: &str, home: Option<&str>) -> String {
    if root.is_empty() {
        return "Unknown project".to_string();
    }
    let normalized = normalized_path(root);
    let home = home.map(normalized_path);
    if home.as_deref() == Some(normalized.as_str()) {
        return "Home".to_string();
    }
    let parts: Vec<&str> = root.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    let Some(last) = parts.last().copied() else {
        return "Unknown project".to_string();
    };
    let home_folder = home.as_deref().and_then(|h| h.rsplit('/').next()).map(str::to_lowercase);
    let name = if parts.len() >= 2 && GENERIC_FOLDERS.contains(&last.to_lowercase().as_str()) {
        let parent = parts[parts.len() - 2];
        if home_folder.as_deref() == Some(parent.to_lowercase().as_str()) || parent.ends_with(':') {
            last.to_string()
        } else {
            format!("{parent}/{last}")
        }
    } else {
        last.to_string()
    };
    name.chars().filter(|c| !c.is_control()).take(MAX_NAME_CHARS).collect()
}

// ---------------------------------------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NamedId {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityRow {
    pub day: String,
    pub account: String,
    pub project: String,
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub cost_inr: Option<f64>,
    pub account_name: String,
    pub project_name: String,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub turns: u64,
    pub unpriced_turns: u64,
    pub priced_cost_inr: f64,
    pub plan_share_inr: Option<f64>,
    pub estimated_spend_inr: Option<f64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct CostValue {
    pub amount_inr: f64,
    pub coverage: Coverage,
    pub unknown_turns: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    /// Nothing to report.
    #[default]
    None,
    /// Every turn priced; zero here is a true zero.
    Complete,
    /// Some priced, some not: the amount is a lower bound.
    AtLeast,
    /// None priced.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AccountBudget {
    pub account: String,
    pub account_name: String,
    pub billing: Billing,
    pub plan_name: Option<String>,
    pub monthly_inr: Option<f64>,
    pub monthly_source: &'static str,
    pub allocation: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivitySession {
    pub id: String,
    pub account: String,
    pub project: String,
    pub project_name: String,
    pub branch: Option<String>,
    pub first_ts: i64,
    pub last_ts: i64,
    pub duration_seconds: i64,
    pub models: Vec<String>,
    pub turns: u64,
    pub tokens: u64,
    pub cost_inr: Option<f64>,
    pub estimated_spend_inr: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ActivityReport {
    pub range: String,
    pub from: String,
    pub to: String,
    pub accounts: Vec<NamedId>,
    pub projects: Vec<NamedId>,
    pub rows: Vec<ActivityRow>,
    pub total_cost_inr: Option<f64>,
    pub unpriced_turns: u64,
    pub partial: bool,
    pub note: String,
    pub coverage: Coverage,
    pub cost_basis: &'static str,
    pub usd_to_inr: f64,
    pub rate_label: &'static str,
    pub unpriced_models: Vec<String>,
    pub estimated_spend: CostValue,
    pub plan_share: CostValue,
    pub api_key_usage: CostValue,
    pub budgets: Vec<AccountBudget>,
    pub sessions: Vec<ActivitySession>,
    pub sessions_truncated: bool,
    pub scan: ScanStats,
}

/// A sum that knows how complete it is.
#[derive(Debug, Clone, Copy, Default)]
struct Tally {
    amount: f64,
    priced: u64,
    unknown: u64,
}

impl Tally {
    fn add(&mut self, cost: Option<f64>) {
        match cost.filter(|c| c.is_finite() && *c >= 0.0) {
            Some(cost) => {
                self.amount += cost;
                self.priced += 1;
            }
            None => self.unknown += 1,
        }
    }
    /// The amount when every turn is priced. A row with any unpriced turn has no single price.
    fn exact(&self) -> Option<f64> {
        (self.unknown == 0 && self.priced > 0).then_some(self.amount)
    }
    fn coverage(&self) -> Coverage {
        match (self.unknown, self.priced) {
            (0, 0) => Coverage::None,
            (0, _) => Coverage::Complete,
            (_, 0) => Coverage::Unavailable,
            _ => Coverage::AtLeast,
        }
    }
    fn value(&self) -> CostValue {
        CostValue { amount_inr: self.amount, coverage: self.coverage(), unknown_turns: self.unknown }
    }
}

#[derive(Debug, Clone)]
struct AccountSource {
    id: String,
    name: String,
    format: Format,
    roots: Vec<PathBuf>,
}

/// Claude accounts from the directories the app already tracks; Codex's default plus `~/.codex-<slug>`.
fn discover_sources(home: Option<&Path>, claude_dirs: &[PathBuf]) -> Vec<AccountSource> {
    let mut out = Vec::new();
    for dir in claude_dirs {
        let slug = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .and_then(|n| n.strip_prefix(".claude-").map(str::to_string));
        out.push(AccountSource {
            id: slug.as_ref().map_or_else(|| "claude".to_string(), |s| format!("claude-{s}")),
            name: slug.as_ref().map_or_else(|| "Claude".to_string(), |s| format!("Claude ({s})")),
            format: Format::Claude,
            roots: vec![dir.join("projects")],
        });
    }
    let Some(home) = home else {
        return out;
    };
    fn codex(dir: &Path, slug: Option<&str>) -> AccountSource {
        AccountSource {
            id: slug.map_or_else(|| "codex".to_string(), |s| format!("codex-{s}")),
            name: slug.map_or_else(|| "Codex".to_string(), |s| format!("Codex ({s})")),
            format: Format::Codex,
            roots: vec![dir.join("sessions"), dir.join("archived_sessions")],
        }
    }
    out.push(codex(&home.join(".codex"), None));
    let mut extras: Vec<(String, PathBuf)> = std::fs::read_dir(home)
        .map(|listing| {
            listing
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let slug = name.strip_prefix(".codex-")?.to_string();
                    let path = entry.path();
                    let real = !slug.is_empty()
                        && entry.file_type().map(|k| k.is_dir()).unwrap_or(false)
                        && (path.join("auth.json").is_file() || path.join("sessions").is_dir());
                    real.then_some((slug, path))
                })
                .collect()
        })
        .unwrap_or_default();
    extras.sort();
    for (slug, path) in &extras {
        out.push(codex(path, Some(slug.as_str())));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Range {
    Day,
    Week,
    Month,
}

impl Range {
    fn parse(value: &str) -> Option<Range> {
        match value {
            "day" => Some(Range::Day),
            "week" => Some(Range::Week),
            "month" => Some(Range::Month),
            _ => None,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Range::Day => "day",
            Range::Week => "week",
            Range::Month => "month",
        }
    }
}

fn month_start(date: NaiveDate) -> NaiveDate {
    date.with_day(1).unwrap_or(date)
}

fn next_month(date: NaiveDate) -> Option<NaiveDate> {
    let (year, month) = if date.month() == 12 { (date.year() + 1, 1) } else { (date.year(), date.month() + 1) };
    NaiveDate::from_ymd_opt(year, month, 1)
}

/// The local dates [from, until) of the period holding `anchor`. Weeks start on Monday.
fn period(range: Range, anchor: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    match range {
        Range::Day => Some((anchor, anchor.checked_add_days(Days::new(1))?)),
        Range::Week => {
            let from = anchor.checked_sub_days(Days::new(anchor.weekday().num_days_from_monday() as u64))?;
            Some((from, from.checked_add_days(Days::new(7))?))
        }
        Range::Month => {
            let from = month_start(anchor);
            Some((from, next_month(from)?))
        }
    }
}

fn date_text(date: NaiveDate) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day())
}

/// The first moment of a local date. A clock change can skip midnight, so the next hours are tried too.
fn local_start<Tz: TimeZone>(zone: &Tz, date: NaiveDate) -> i64 {
    for hour in 0..4 {
        if let Some(moment) = date
            .and_hms_opt(hour, 0, 0)
            .and_then(|naive| zone.from_local_datetime(&naive).earliest())
        {
            return moment.timestamp();
        }
    }
    0
}

/// Everything a report reads, all of it injected, so a test never meets a real profile.
struct ReportInput<'a> {
    sources: &'a [AccountSource],
    settings: &'a CostSettings,
    prices: &'a PriceTable,
    plans: &'a BTreeMap<String, PlanInfo>,
    home: Option<&'a str>,
    limits: &'a Limits,
}

struct Query<'a> {
    range: &'a str,
    account: Option<&'a str>,
    project: Option<&'a str>,
    date: Option<&'a str>,
}

#[derive(Default)]
struct RowTotals {
    usage: Usage,
    turns: u64,
    list: Tally,
    plan: Tally,
}

#[derive(Default)]
struct SessionTotals {
    branch: Option<String>,
    first: i64,
    last: i64,
    models: BTreeSet<String>,
    usage: Usage,
    turns: u64,
    list: Tally,
    spend: Tally,
    /// A session spanning days counts each day's stretch, so an overnight gap is not "work".
    days: BTreeMap<NaiveDate, (i64, i64)>,
}

impl Engine {
    fn report<Tz: TimeZone>(&self, input: &ReportInput<'_>, query: &Query<'_>, now: DateTime<Tz>) -> Result<ActivityReport, String> {
        let range = Range::parse(query.range).ok_or_else(|| "Range must be day, week or month".to_string())?;
        let anchor = match query.date.filter(|d| !d.is_empty()) {
            Some(text) => NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| "Date must be YYYY-MM-DD".to_string())?,
            None => now.date_naive(),
        };
        let (from, until) = period(range, anchor).ok_or_else(|| "Date is out of range".to_string())?;
        let zone = now.timezone();
        let last_day = until.checked_sub_days(Days::new(1)).unwrap_or(from);

        // A plan's month is split over that whole month's tokens, so the whole month has to be read even for
        // one day, across both months if a week straddles them.
        let scan_from = month_start(from);
        let scan_until = next_month(last_day).ok_or_else(|| "Date is out of range".to_string())?;
        let since = local_start(&zone, scan_from) - MTIME_SLACK_SECONDS;
        let report_from = local_start(&zone, from) - MTIME_SLACK_SECONDS;

        let account_filter = query.account.filter(|a| !a.is_empty());
        let project_filter = query.project.filter(|p| !p.is_empty());
        let rate = input.settings.usd_to_inr;

        let mut stats = ScanStats::default();
        let mut rows: BTreeMap<(NaiveDate, usize, String, String), RowTotals> = BTreeMap::new();
        let mut sessions: HashMap<(usize, String, String), SessionTotals> = HashMap::new();
        let mut budgets = Vec::new();
        let mut list_total = Tally::default();
        let mut plan_total = Tally::default();
        let mut api_total = Tally::default();
        let mut unpriced_models: BTreeSet<String> = BTreeSet::new();
        let mut project_ids: BTreeMap<String, String> = BTreeMap::new(); // id -> a root

        for (index, source) in input.sources.iter().enumerate() {
            let settings = input.settings.accounts.get(&source.id).cloned().unwrap_or_default();
            let plan = settings.plan_tier.as_ref().and_then(|tier| input.plans.get(tier));
            let list_monthly = plan.and_then(|p| plan_monthly_inr(p, rate));
            let monthly = settings.monthly_paid_inr.or(list_monthly);
            budgets.push(AccountBudget {
                account: source.id.clone(),
                account_name: source.name.clone(),
                billing: settings.billing,
                plan_name: plan.map(|p| p.name.clone()),
                monthly_inr: if settings.billing == Billing::Subscription { monthly } else { None },
                monthly_source: match (settings.billing, settings.monthly_paid_inr, list_monthly) {
                    (Billing::Api, _, _) => "unknown",
                    (_, Some(_), _) => "you_entered",
                    (_, None, Some(_)) => "plan_list_price",
                    _ => "unknown",
                },
                allocation: if settings.billing == Billing::Subscription { "month_token_share" } else { "none" },
            });
            if account_filter.map_or(false, |wanted| wanted != source.id) {
                continue;
            }

            let turns = self.account_turns(source, since, report_from, input.limits, &mut stats);
            let dated: Vec<(NaiveDate, Turn)> = turns
                .into_iter()
                .filter_map(|turn| {
                    let day = DateTime::from_timestamp(turn.ts, 0)?.with_timezone(&zone).date_naive();
                    (day >= scan_from && day < scan_until).then_some((day, turn))
                })
                .collect();

            // Computed before any project filter: a project's share of the plan does not change by looking
            // at it alone.
            let mut month_weights: BTreeMap<(i32, u32), f64> = BTreeMap::new();
            for (day, turn) in &dated {
                *month_weights.entry((day.year(), day.month())).or_default() += turn.usage.weight();
            }

            for (day, turn) in &dated {
                if *day < from || *day >= until {
                    continue;
                }
                let id = project_id(&turn.project);
                project_ids.entry(id.clone()).or_insert_with(|| turn.project.to_string());
                if project_filter.map_or(false, |wanted| wanted != id) {
                    continue;
                }
                let list = input.prices.inr(&turn.model, &turn.usage, rate);
                let plan_share = match settings.billing {
                    Billing::Api => None,
                    Billing::Subscription => {
                        let weight = turn.usage.weight();
                        if weight <= 0.0 {
                            Some(0.0)
                        } else {
                            month_weights
                                .get(&(day.year(), day.month()))
                                .filter(|w| **w > 0.0)
                                .and_then(|month| monthly.map(|m| m * weight / month))
                        }
                    }
                };
                list_total.add(list);
                if list.is_none() {
                    unpriced_models.insert(turn.model.to_string());
                }
                match settings.billing {
                    Billing::Subscription => plan_total.add(plan_share),
                    Billing::Api => api_total.add(list),
                }

                let row = rows.entry((*day, index, id.clone(), turn.model.to_string())).or_default();
                row.usage.add(&turn.usage);
                row.turns += 1;
                row.list.add(list);
                if settings.billing == Billing::Subscription {
                    row.plan.add(plan_share);
                }

                let session = sessions
                    .entry((index, turn.session.to_string(), id))
                    .or_insert_with(|| SessionTotals { first: turn.ts, last: turn.ts, ..SessionTotals::default() });
                session.first = session.first.min(turn.ts);
                session.last = session.last.max(turn.ts);
                if let Some(branch) = &turn.branch {
                    session.branch = Some(branch.to_string());
                }
                session.models.insert(turn.model.to_string());
                session.usage.add(&turn.usage);
                session.turns += 1;
                session.list.add(list);
                session.spend.add(if settings.billing == Billing::Subscription { plan_share } else { list });
                let stretch = session.days.entry(*day).or_insert((turn.ts, turn.ts));
                stretch.0 = stretch.0.min(turn.ts);
                stretch.1 = stretch.1.max(turn.ts);
            }
        }

        let names = project_names(&project_ids, input.home);
        let name_of = |id: &str| names.get(id).cloned().unwrap_or_else(|| "Unknown project".to_string());

        let out_rows: Vec<ActivityRow> = rows
            .into_iter()
            .map(|((day, index, project, model), row)| {
                let billing = input
                    .settings
                    .accounts
                    .get(&input.sources[index].id)
                    .map(|a| a.billing)
                    .unwrap_or_default();
                let plan_share = if billing == Billing::Subscription { row.plan.exact() } else { None };
                ActivityRow {
                    day: date_text(day),
                    account: input.sources[index].id.clone(),
                    account_name: input.sources[index].name.clone(),
                    project_name: name_of(&project),
                    project,
                    model,
                    input_tokens: row.usage.input,
                    output_tokens: row.usage.output,
                    cached_tokens: row.usage.cache_read.saturating_add(row.usage.cache_write),
                    cache_read_tokens: row.usage.cache_read,
                    cache_write_tokens: row.usage.cache_write,
                    cost_inr: row.list.exact(),
                    turns: row.turns,
                    unpriced_turns: row.list.unknown,
                    priced_cost_inr: row.list.amount,
                    plan_share_inr: plan_share,
                    estimated_spend_inr: if billing == Billing::Subscription { plan_share } else { row.list.exact() },
                }
            })
            .collect();

        let mut out_sessions: Vec<ActivitySession> = sessions
            .into_iter()
            .map(|((index, session, project), totals)| ActivitySession {
                id: session.chars().take(8).collect(),
                account: input.sources[index].id.clone(),
                project_name: name_of(&project),
                project,
                branch: totals.branch,
                first_ts: totals.first,
                last_ts: totals.last,
                duration_seconds: totals.days.values().map(|(a, b)| (b - a).max(MIN_SLICE_SECONDS)).sum(),
                models: totals.models.into_iter().collect(),
                turns: totals.turns,
                tokens: totals.usage.tokens(),
                cost_inr: totals.list.exact(),
                estimated_spend_inr: totals.spend.exact(),
            })
            .collect();
        out_sessions.sort_by(|a, b| b.last_ts.cmp(&a.last_ts).then_with(|| a.id.cmp(&b.id)).then_with(|| a.project.cmp(&b.project)));
        let sessions_truncated = out_sessions.len() > MAX_SESSIONS;
        out_sessions.truncate(MAX_SESSIONS);

        let accounts = input
            .sources
            .iter()
            .map(|s| NamedId { id: s.id.clone(), name: s.name.clone() })
            .collect();
        let mut projects: Vec<NamedId> = project_ids.keys().map(|id| NamedId { id: id.clone(), name: name_of(id) }).collect();
        projects.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));

        let mut spend_total = plan_total;
        spend_total.amount += api_total.amount;
        spend_total.priced += api_total.priced;
        spend_total.unknown += api_total.unknown;

        let incomplete = stats.files_skipped > 0 || stats.lines_skipped > 0;
        let partial = list_total.unknown > 0 || incomplete;
        Ok(ActivityReport {
            range: range.as_str().to_string(),
            from: date_text(from),
            to: date_text(last_day),
            accounts,
            projects,
            rows: out_rows,
            total_cost_inr: match list_total.coverage() {
                Coverage::Complete | Coverage::AtLeast => Some(list_total.amount),
                Coverage::None | Coverage::Unavailable => None,
            },
            unpriced_turns: list_total.unknown,
            partial,
            note: report_note(rate, &list_total, &stats),
            coverage: list_total.coverage(),
            cost_basis: COST_BASIS,
            usd_to_inr: rate,
            rate_label: RATE_LABEL,
            unpriced_models: unpriced_models.into_iter().collect(),
            estimated_spend: spend_total.value(),
            plan_share: plan_total.value(),
            api_key_usage: api_total.value(),
            budgets,
            sessions: out_sessions,
            sessions_truncated,
            scan: stats,
        })
    }
}

/// Names for the report's projects. Two different folders can share a name, so those get a short id each.
fn project_names(project_ids: &BTreeMap<String, String>, home: Option<&str>) -> HashMap<String, String> {
    let mut seen: HashMap<String, u32> = HashMap::new();
    let plain: Vec<(String, String)> = project_ids
        .iter()
        .map(|(id, root)| {
            let name = project_name(root, home);
            *seen.entry(name.to_lowercase()).or_default() += 1;
            (id.clone(), name)
        })
        .collect();
    plain
        .into_iter()
        .map(|(id, name)| {
            let shared = seen.get(&name.to_lowercase()).copied().unwrap_or(0) > 1;
            let shown = if shared {
                let short: String = id.chars().take(4).collect();
                format!("{name} ({short})")
            } else {
                name
            };
            (id, shown)
        })
        .collect()
}

fn report_note(rate: f64, list: &Tally, stats: &ScanStats) -> String {
    let mut note = format!(
        "Estimates in INR, not a bill. Cost is the API-equivalent: tokens at list price, converted at your rate of 1 USD = ₹{rate:.2}, which is your estimate and not a live exchange rate. On a monthly plan this is a comparison, not money spent; estimated spend is that month's plan amount split over that month's local tokens."
    );
    if list.unknown > 0 {
        note.push_str(&format!(" Turns with no known price: {}. They are not counted, so the total is a minimum.", list.unknown));
    }
    if stats.files_skipped > 0 {
        note.push_str(&format!(" Transcript files skipped because of read limits or errors: {}.", stats.files_skipped));
    }
    if stats.lines_skipped > 0 {
        note.push_str(&format!(" Transcript lines that could not be read: {}.", stats.lines_skipped));
    }
    note
}

// ---------------------------------------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------------------------------------

fn costs_dir() -> PathBuf {
    crate::config::config_path()
        .parent()
        .map(|parent| parent.join("costs"))
        .unwrap_or_else(|| PathBuf::from("costs"))
}

fn settings_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(Engine::default)
}

#[tauri::command]
pub fn get_cost_settings() -> CostSettings {
    load_settings(&costs_dir())
}

#[tauri::command]
pub fn set_cost_settings(settings: CostSettings) -> Result<CostSettings, String> {
    let _guard = settings_lock().lock().unwrap_or_else(|e| e.into_inner());
    save_settings(&costs_dir(), settings)
}

#[tauri::command]
pub fn get_cost_plans() -> Vec<PlanInfo> {
    load_plans(&costs_dir()).into_values().collect()
}

#[tauri::command(async)]
pub fn get_activity_report(
    range: String,
    account: Option<String>,
    project: Option<String>,
    date: Option<String>,
) -> Result<ActivityReport, String> {
    let dir = costs_dir();
    let settings = load_settings(&dir);
    let prices = price_table(&dir, &settings);
    let plans = load_plans(&dir);
    let home = dirs::home_dir();
    let sources = discover_sources(home.as_deref(), &crate::usage::profile_dirs());
    let home_text = home.as_ref().map(|h| h.to_string_lossy().into_owned());
    let limits = Limits::default();
    let input = ReportInput {
        sources: &sources,
        settings: &settings,
        prices: &prices,
        plans: &plans,
        home: home_text.as_deref(),
        limits: &limits,
    };
    let query = Query { range: &range, account: account.as_deref(), project: project.as_deref(), date: date.as_deref() };
    engine().report(&input, &query, Local::now())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PriceRefresh {
    pub models: usize,
    pub updated_at: i64,
}

/// Runs only when the owner asks. Sends no key and no account data, and leaves the exchange rate alone.
#[tauri::command(async)]
pub fn refresh_cost_prices() -> Result<PriceRefresh, String> {
    const FAILED: &str = "Could not refresh prices";
    let response = crate::http::agent().get(OPENROUTER_MODELS_URL).call().map_err(|_| FAILED.to_string())?;
    let mut raw = Vec::new();
    response
        .into_reader()
        .take(MAX_REMOTE_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| FAILED.to_string())?;
    if raw.len() as u64 > MAX_REMOTE_BYTES {
        return Err(FAILED.to_string());
    }
    let prices = parse_openrouter(&raw);
    if prices.is_empty() {
        return Err(FAILED.to_string());
    }
    let updated_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let body = serde_json::json!({ "prices": prices, "prices_updated_at": updated_at, "source": "openrouter" });
    let text = serde_json::to_vec_pretty(&body).map_err(|_| FAILED.to_string())?;
    let _guard = settings_lock().lock().unwrap_or_else(|e| e.into_inner());
    atomic_write(&costs_dir(), "prices.json", &text)?;
    Ok(PriceRefresh { models: prices.len(), updated_at })
}

// ---------------------------------------------------------------------------------------------------------
// Tests: fixtures only, under a temporary folder. No test reads a real profile or the network.
// ---------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> TempDir {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "matra-costs-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&path).unwrap();
            TempDir(path)
        }
        fn write(&self, relative: &str, content: &str) -> PathBuf {
            let path = self.0.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn ist() -> FixedOffset {
        FixedOffset::east_opt(5 * 3600 + 30 * 60).unwrap()
    }

    /// Fixtures carry old dates on freshly written files, so nothing is dropped by the file age check.
    fn now_ist(text: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(text).unwrap().with_timezone(&ist())
    }

    const NOW: &str = "2026-10-15T12:00:00+05:30";

    #[allow(clippy::too_many_arguments)]
    fn claude_line(request: &str, session: &str, stamp: &str, model: &str, cwd: &str, input: u64, output: u64, read: u64, write: u64) -> String {
        serde_json::json!({
            "type": "assistant",
            "timestamp": stamp,
            "sessionId": session,
            "requestId": request,
            "cwd": cwd,
            "gitBranch": "main",
            "message": {
                "model": model,
                "role": "assistant",
                "content": [{"type": "text", "text": "SECRET-ANSWER-TEXT"}],
                "usage": {
                    "input_tokens": input,
                    "output_tokens": output,
                    "cache_read_input_tokens": read,
                    "cache_creation_input_tokens": write
                }
            }
        })
        .to_string()
    }

    fn user_line(text: &str) -> String {
        serde_json::json!({"type": "user", "cwd": "/x", "message": {"role": "user", "content": text}}).to_string()
    }

    fn codex_meta(id: &str, cwd: &str) -> String {
        serde_json::json!({"timestamp": "2026-10-15T01:00:00Z", "type": "session_meta", "payload": {"id": id, "cwd": cwd}}).to_string()
    }

    fn codex_context(model: &str, cwd: &str) -> String {
        serde_json::json!({"timestamp": "2026-10-15T01:00:00Z", "type": "turn_context", "payload": {"model": model, "cwd": cwd}}).to_string()
    }

    fn counters(input: u64, cached: u64, output: u64) -> Value {
        serde_json::json!({"input_tokens": input, "cached_input_tokens": cached, "output_tokens": output, "reasoning_output_tokens": 7, "total_tokens": input + output})
    }

    fn codex_tick(stamp: &str, total: Option<Value>, last: Option<Value>) -> String {
        let mut info = serde_json::Map::new();
        if let Some(total) = total {
            info.insert("total_token_usage".to_string(), total);
        }
        if let Some(last) = last {
            info.insert("last_token_usage".to_string(), last);
        }
        serde_json::json!({"timestamp": stamp, "type": "event_msg", "payload": {"type": "token_count", "info": info}}).to_string()
    }

    fn lines(items: &[String]) -> String {
        let mut out = items.join("\n");
        out.push('\n');
        out
    }

    fn source(id: &str, format: Format, root: PathBuf) -> AccountSource {
        AccountSource { id: id.to_string(), name: id.to_uppercase(), format, roots: vec![root] }
    }

    struct Fixture {
        settings: CostSettings,
        prices: PriceTable,
        plans: BTreeMap<String, PlanInfo>,
        limits: Limits,
        home: Option<String>,
    }

    impl Fixture {
        fn new() -> Fixture {
            let mut prices = PriceTable::default();
            prices.add(prices_from_json(PRICES_DEFAULT.as_bytes()));
            Fixture {
                settings: CostSettings::default(),
                prices,
                plans: plans_from_json(PLANS_DEFAULT.as_bytes()).unwrap(),
                limits: Limits::default(),
                home: None,
            }
        }
        #[allow(clippy::too_many_arguments)]
        fn report(&self, engine: &Engine, sources: &[AccountSource], range: &str, account: Option<&str>, project: Option<&str>, date: Option<&str>, now: &str) -> ActivityReport {
            let input = ReportInput {
                sources,
                settings: &self.settings,
                prices: &self.prices,
                plans: &self.plans,
                home: self.home.as_deref(),
                limits: &self.limits,
            };
            let query = Query { range, account, project, date };
            engine.report(&input, &query, now_ist(now)).unwrap()
        }
        fn month(&self, sources: &[AccountSource]) -> ActivityReport {
            self.report(&Engine::default(), sources, "month", None, None, None, NOW)
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_session_changing_model_prices_each_turn_at_its_own_model() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[
            user_line("hello"),
            claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 1_000_000, 0, 0, 0),
            claude_line("r2", "s1", "2026-10-15T02:00:00Z", "claude-haiku-4", "/p", 1_000_000, 0, 0, 0),
        ]));
        let fixture = Fixture::new();
        let report = fixture.month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert_eq!(report.rows.len(), 2);
        let cost = |model: &str| report.rows.iter().find(|r| r.model == model).unwrap().cost_inr.unwrap();
        assert!(close(cost("claude-opus-5"), 5.0 * 84.0));
        assert!(close(cost("claude-haiku-4"), 1.0 * 84.0));
        assert!(close(report.total_cost_inr.unwrap(), 6.0 * 84.0));
        assert_eq!(report.coverage, Coverage::Complete);
        assert!(!report.partial);
        assert_eq!(report.cost_basis, COST_BASIS);
    }

    #[test]
    fn cached_tokens_use_their_own_rates_and_stay_separate() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[claude_line(
            "r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 1_000_000, 1_000_000, 2_000_000, 4_000_000,
        )]));
        let fixture = Fixture::new();
        let report = fixture.month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        let row = &report.rows[0];
        assert_eq!(row.input_tokens, 1_000_000);
        assert_eq!(row.output_tokens, 1_000_000);
        assert_eq!(row.cache_read_tokens, 2_000_000);
        assert_eq!(row.cache_write_tokens, 4_000_000);
        assert_eq!(row.cached_tokens, 6_000_000);
        let usd = 5.0 + 25.0 + 2.0 * 0.5 + 4.0 * 6.25;
        assert!(close(row.cost_inr.unwrap(), usd * 84.0));
    }

    #[test]
    fn a_missing_cache_rate_is_unpriced_not_free() {
        let mut table = PriceTable::default();
        table.add([ModelPrice { model: "m".into(), input: 1.0, output: 1.0, cache_read: None, cache_write: None }]);
        let plain = Usage { input: 1_000_000, ..Usage::default() };
        let cached = Usage { input: 1_000_000, cache_read: 1, ..Usage::default() };
        assert!(close(table.inr("m", &plain, 84.0).unwrap(), 84.0));
        assert_eq!(table.inr("m", &cached, 84.0), None);
        assert_eq!(table.inr("m", &Usage::default(), 84.0), Some(0.0));
        assert_eq!(table.inr("nobody", &Usage::default(), 84.0), Some(0.0));
    }

    #[test]
    fn streaming_repeats_and_resumed_copies_count_once_at_final_size() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 100, 10, 0, 0),
            claude_line("r1", "s1", "2026-10-15T01:00:05Z", "claude-opus-5", "/p", 100, 90, 0, 0),
        ]));
        dir.write("projects/-p/b.jsonl", &lines(&[
            claude_line("r1", "s2", "2026-10-15T01:00:05Z", "claude-opus-5", "/p", 100, 90, 0, 0),
            claude_line("r2", "s2", "2026-10-15T03:00:00Z", "claude-opus-5", "/p", 1, 1, 0, 0),
        ]));
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert_eq!(report.rows[0].turns, 2);
        assert_eq!(report.rows[0].input_tokens, 101);
        assert_eq!(report.rows[0].output_tokens, 91);
    }

    #[test]
    fn synthetic_turns_are_not_usage() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[claude_line("r1", "s1", "2026-10-15T01:00:00Z", "<synthetic>", "/p", 5, 5, 0, 0)]));
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert!(report.rows.is_empty());
        assert_eq!(report.coverage, Coverage::None);
        assert_eq!(report.total_cost_inr, None);
        assert_eq!(report.scan.lines_skipped, 0);
    }

    #[test]
    fn codex_running_totals_are_counted_as_deltas() {
        let dir = TempDir::new();
        dir.write("sessions/2026/10/15/rollout-a.jsonl", &lines(&[
            codex_meta("sess-1", "/p"),
            codex_context("gpt-5", "/p"),
            codex_tick("2026-10-15T02:00:00.100Z", Some(counters(1000, 200, 100)), Some(counters(1000, 200, 100))),
            // Codex repeats a tick unchanged: not a new turn.
            codex_tick("2026-10-15T02:00:01.100Z", Some(counters(1000, 200, 100)), Some(counters(1000, 200, 100))),
            codex_tick("2026-10-15T02:05:00.100Z", Some(counters(1500, 300, 150)), Some(counters(500, 100, 50))),
            // A tick with no info is a rate-limit reading.
            serde_json::json!({"timestamp": "2026-10-15T02:06:00Z", "type": "event_msg", "payload": {"type": "token_count", "info": null}}).to_string(),
        ]));
        let report = Fixture::new().month(&[source("codex", Format::Codex, dir.0.join("sessions"))]);
        assert_eq!(report.rows.len(), 1);
        let row = &report.rows[0];
        assert_eq!(row.turns, 2);
        assert_eq!(row.input_tokens, 1200); // 1500 total, less 300 cached
        assert_eq!(row.cache_read_tokens, 300);
        assert_eq!(row.output_tokens, 150); // reasoning is part of output, not added again
        let usd = 1200.0 * 1.25 + 150.0 * 10.0 + 300.0 * 0.125;
        assert!(close(row.cost_inr.unwrap(), usd / 1e6 * 84.0));
    }

    #[test]
    fn a_resumed_rollouts_first_total_does_not_recount_earlier_work() {
        let dir = TempDir::new();
        dir.write("sessions/a.jsonl", &lines(&[
            codex_meta("sess-1", "/p"),
            codex_context("gpt-5", "/p"),
            codex_tick("2026-10-15T02:00:00Z", Some(counters(900_000, 0, 90_000)), Some(counters(100, 0, 10))),
            codex_tick("2026-10-15T02:01:00Z", Some(counters(900_200, 0, 90_020)), Some(counters(200, 0, 20))),
        ]));
        let report = Fixture::new().month(&[source("codex", Format::Codex, dir.0.join("sessions"))]);
        assert_eq!(report.rows[0].input_tokens, 300);
        assert_eq!(report.rows[0].output_tokens, 30);
    }

    #[test]
    fn a_running_total_that_resets_starts_again_instead_of_going_negative() {
        let dir = TempDir::new();
        dir.write("sessions/a.jsonl", &lines(&[
            codex_context("gpt-5", "/p"),
            codex_tick("2026-10-15T02:00:00Z", Some(counters(1000, 0, 100)), None),
            codex_tick("2026-10-15T02:01:00Z", Some(counters(50, 0, 5)), None),
            codex_tick("2026-10-15T02:02:00Z", Some(counters(70, 0, 8)), None),
        ]));
        let report = Fixture::new().month(&[source("codex", Format::Codex, dir.0.join("sessions"))]);
        assert_eq!(report.rows[0].input_tokens, 1000 + 50 + 20);
        assert_eq!(report.rows[0].output_tokens, 100 + 5 + 3);
    }

    #[test]
    fn older_rollouts_with_only_last_usage_collapse_exact_repeats() {
        let dir = TempDir::new();
        dir.write("sessions/a.jsonl", &lines(&[
            codex_meta("sess-1", "/p"),
            codex_context("gpt-5", "/p"),
            codex_tick("2026-10-15T02:00:00Z", None, Some(counters(100, 40, 10))),
            codex_tick("2026-10-15T02:00:00Z", None, Some(counters(100, 40, 10))),
            codex_tick("2026-10-15T02:03:00Z", None, Some(counters(100, 40, 10))),
        ]));
        let report = Fixture::new().month(&[source("codex", Format::Codex, dir.0.join("sessions"))]);
        assert_eq!(report.rows[0].turns, 2);
        assert_eq!(report.rows[0].input_tokens, 120);
        assert_eq!(report.rows[0].cache_read_tokens, 80);
    }

    #[test]
    fn a_forked_rollouts_copied_ticks_are_not_counted_twice() {
        let dir = TempDir::new();
        let parent = [
            codex_tick("2026-10-15T02:00:00.500Z", Some(counters(100, 0, 10)), Some(counters(100, 0, 10))),
            codex_tick("2026-10-15T02:01:00.500Z", Some(counters(300, 0, 30)), Some(counters(200, 0, 20))),
        ];
        let mut first = vec![codex_meta("parent", "/p"), codex_context("gpt-5", "/p")];
        first.extend_from_slice(&parent);
        dir.write("sessions/parent.jsonl", &lines(&first));
        let mut second = vec![codex_meta("child", "/p"), codex_meta("parent", "/p"), codex_context("gpt-5", "/p")];
        second.extend_from_slice(&parent);
        second.push(codex_tick("2026-10-15T03:00:00.500Z", Some(counters(350, 0, 35)), Some(counters(50, 0, 5))));
        dir.write("sessions/child.jsonl", &lines(&second));
        let report = Fixture::new().month(&[source("codex", Format::Codex, dir.0.join("sessions"))]);
        assert_eq!(report.rows[0].input_tokens, 350);
        assert_eq!(report.rows[0].output_tokens, 35);
    }

    #[test]
    fn each_codex_turn_uses_the_model_named_before_it() {
        let dir = TempDir::new();
        dir.write("sessions/a.jsonl", &lines(&[
            codex_meta("sess-1", "/p"),
            codex_context("gpt-5", "/p"),
            codex_tick("2026-10-15T02:00:00Z", Some(counters(100, 0, 10)), None),
            // Another agent's model named in passing must not re-label this session.
            r#"{"timestamp":"2026-10-15T02:00:30Z","type":"event_msg","payload":{"type":"agent_spawn","model":"gpt-6"}}"#.to_string(),
            codex_tick("2026-10-15T02:01:00Z", Some(counters(200, 0, 20)), None),
            codex_context("gpt-6", "/p"),
            codex_tick("2026-10-15T02:02:00Z", Some(counters(300, 0, 30)), None),
        ]));
        let report = Fixture::new().month(&[source("codex", Format::Codex, dir.0.join("sessions"))]);
        let tokens = |model: &str| report.rows.iter().find(|r| r.model == model).unwrap().input_tokens;
        assert_eq!(tokens("gpt-5"), 200);
        assert_eq!(tokens("gpt-6"), 100);
    }

    #[test]
    fn a_model_named_elsewhere_is_used_until_a_turn_context_arrives() {
        let dir = TempDir::new();
        dir.write("sessions/a.jsonl", &lines(&[
            r#"{"timestamp":"2026-10-15T02:00:00Z","type":"event_msg","payload":{"type":"task_started","model":"gpt-5"}}"#.to_string(),
            codex_tick("2026-10-15T02:00:30Z", Some(counters(100, 0, 10)), None),
        ]));
        let report = Fixture::new().month(&[source("codex", Format::Codex, dir.0.join("sessions"))]);
        assert_eq!(report.rows[0].model, "gpt-5");
    }

    #[test]
    fn codex_with_no_model_or_cwd_is_visible_and_unpriced() {
        let dir = TempDir::new();
        dir.write("sessions/rollout-x.jsonl", &lines(&[codex_tick("2026-10-15T02:00:00Z", Some(counters(100, 0, 10)), None)]));
        let report = Fixture::new().month(&[source("codex", Format::Codex, dir.0.join("sessions"))]);
        assert_eq!(report.rows[0].model, "codex");
        assert_eq!(report.rows[0].cost_inr, None);
        assert_eq!(report.rows[0].project, UNKNOWN_PROJECT_ID);
        assert_eq!(report.rows[0].project_name, "Unknown project");
        assert_eq!(report.sessions[0].id, "rollout-");
    }

    #[test]
    fn an_unknown_model_is_unknown_never_zero() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-future-9", "/p", 100, 10, 0, 0)]));
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert_eq!(report.rows[0].cost_inr, None);
        assert_eq!(report.total_cost_inr, None);
        assert_eq!(report.coverage, Coverage::Unavailable);
        assert_eq!(report.unpriced_turns, 1);
        assert!(report.partial);
        assert_eq!(report.unpriced_models, vec!["claude-future-9"]);
        assert!(report.note.contains("Turns with no known price: 1."));
    }

    #[test]
    fn some_unknown_makes_the_total_a_minimum() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 1_000_000, 0, 0, 0),
            claude_line("r2", "s1", "2026-10-15T02:00:00Z", "claude-future-9", "/p", 1_000_000, 0, 0, 0),
        ]));
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert_eq!(report.coverage, Coverage::AtLeast);
        assert!(close(report.total_cost_inr.unwrap(), 5.0 * 84.0));
        assert!(report.partial);
        assert_eq!(report.unpriced_turns, 1);
    }

    #[test]
    fn a_row_with_mixed_coverage_has_no_single_price() {
        let mut fixture = Fixture::new();
        fixture.prices = PriceTable::default();
        fixture.prices.add([ModelPrice { model: "m1".into(), input: 1.0, output: 1.0, cache_read: None, cache_write: None }]);
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-10-15T01:00:00Z", "m1", "/p", 1_000_000, 0, 0, 0),
            claude_line("r2", "s1", "2026-10-15T02:00:00Z", "m1", "/p", 1_000_000, 0, 1, 0),
        ]));
        let report = fixture.month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        let row = &report.rows[0];
        assert_eq!(row.cost_inr, None);
        assert_eq!(row.unpriced_turns, 1);
        assert!(close(row.priced_cost_inr, 84.0));
        assert_eq!(report.sessions[0].cost_inr, None);
    }

    #[test]
    fn prices_match_exact_models_after_normalising_and_custom_wins() {
        let mut fixture = Fixture::new();
        let one = Usage { input: 1, ..Usage::default() };
        assert!(fixture.prices.inr("claude-opus-5-5", &one, 84.0).is_none());
        assert!(fixture.prices.inr("anthropic/Claude-Opus.5", &one, 84.0).is_some());
        fixture.settings.custom_prices = vec![ModelPrice { model: "claude-opus-5".into(), input: 1.0, output: 0.0, cache_read: None, cache_write: None }];
        let dir = TempDir::new();
        let table = price_table(&dir.0, &fixture.settings);
        assert!(close(table.inr("claude-opus-5", &Usage { input: 1_000_000, ..Usage::default() }, 84.0).unwrap(), 84.0));
    }

    #[test]
    fn a_refreshed_list_sits_between_bundled_and_custom() {
        let dir = TempDir::new();
        dir.write("prices.json", r#"{"prices":[{"model":"gpt-new","input":2,"output":3},{"model":"claude-opus-5","input":100,"output":100,"cache_read":1,"cache_write":1},{"model":"","input":1,"output":1},{"nonsense":true}]}"#);
        let table = price_table(&dir.0, &CostSettings::default());
        let input = Usage { input: 1_000_000, ..Usage::default() };
        assert!(close(table.inr("gpt-new", &input, 1.0).unwrap(), 2.0));
        assert!(close(table.inr("claude-opus-5", &input, 1.0).unwrap(), 100.0));
        assert!(close(table.inr("claude-sonnet-5", &input, 1.0).unwrap(), 2.0));
    }

    #[test]
    fn day_boundaries_follow_local_midnight() {
        let dir = TempDir::new();
        // IST is +05:30. 18:29:59Z on the 14th is 23:59:59 on the 14th; 18:30:00Z is 00:00:00 on the 15th.
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-10-14T18:29:59Z", "claude-opus-5", "/p", 1, 0, 0, 0),
            claude_line("r2", "s1", "2026-10-14T18:30:00Z", "claude-opus-5", "/p", 2, 0, 0, 0),
            claude_line("r3", "s1", "2026-10-15T18:29:59Z", "claude-opus-5", "/p", 4, 0, 0, 0),
            claude_line("r4", "s1", "2026-10-15T18:30:00Z", "claude-opus-5", "/p", 8, 0, 0, 0),
        ]));
        let sources = [source("claude", Format::Claude, dir.0.join("projects"))];
        let report = Fixture::new().report(&Engine::default(), &sources, "day", None, None, None, NOW);
        assert_eq!((report.from.as_str(), report.to.as_str()), ("2026-10-15", "2026-10-15"));
        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0].day, "2026-10-15");
        assert_eq!(report.rows[0].input_tokens, 6);
    }

    #[test]
    fn week_boundaries_are_monday_to_sunday() {
        let dir = TempDir::new();
        // 2026-10-15 is a Thursday, so the week is Mon 12th to Sun 18th, local time.
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-10-11T18:29:59Z", "claude-opus-5", "/p", 1, 0, 0, 0),
            claude_line("r2", "s1", "2026-10-11T18:30:00Z", "claude-opus-5", "/p", 2, 0, 0, 0),
            claude_line("r3", "s1", "2026-10-18T18:29:59Z", "claude-opus-5", "/p", 4, 0, 0, 0),
            claude_line("r4", "s1", "2026-10-18T18:30:00Z", "claude-opus-5", "/p", 8, 0, 0, 0),
        ]));
        let sources = [source("claude", Format::Claude, dir.0.join("projects"))];
        let report = Fixture::new().report(&Engine::default(), &sources, "week", None, None, None, NOW);
        assert_eq!((report.from.as_str(), report.to.as_str()), ("2026-10-12", "2026-10-18"));
        let tokens: u64 = report.rows.iter().map(|r| r.input_tokens).sum();
        assert_eq!(tokens, 6);
        assert_eq!(report.rows.first().unwrap().day, "2026-10-12");
        assert_eq!(report.rows.last().unwrap().day, "2026-10-18");
    }

    #[test]
    fn month_boundaries_and_date_navigation() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-09-30T18:29:59Z", "claude-opus-5", "/p", 1, 0, 0, 0),
            claude_line("r2", "s1", "2026-09-30T18:30:00Z", "claude-opus-5", "/p", 2, 0, 0, 0),
            claude_line("r3", "s1", "2026-10-31T18:29:59Z", "claude-opus-5", "/p", 4, 0, 0, 0),
            claude_line("r4", "s1", "2026-10-31T18:30:00Z", "claude-opus-5", "/p", 8, 0, 0, 0),
        ]));
        let sources = [source("claude", Format::Claude, dir.0.join("projects"))];
        let fixture = Fixture::new();
        let engine = Engine::default();
        let total = |report: &ActivityReport| report.rows.iter().map(|r| r.input_tokens).sum::<u64>();
        let october = fixture.report(&engine, &sources, "month", None, None, None, NOW);
        assert_eq!((october.from.as_str(), october.to.as_str()), ("2026-10-01", "2026-10-31"));
        assert_eq!(total(&october), 6);
        let september = fixture.report(&engine, &sources, "month", None, None, Some("2026-09-10"), NOW);
        assert_eq!((september.from.as_str(), september.to.as_str()), ("2026-09-01", "2026-09-30"));
        assert_eq!(total(&september), 1);
        let december = fixture.report(&engine, &sources, "month", None, None, Some("2026-12-31"), NOW);
        assert_eq!((december.from.as_str(), december.to.as_str()), ("2026-12-01", "2026-12-31"));
        assert_eq!(total(&december), 0);
    }

    #[test]
    fn a_bad_range_or_date_is_an_error() {
        let fixture = Fixture::new();
        let input = ReportInput { sources: &[], settings: &fixture.settings, prices: &fixture.prices, plans: &fixture.plans, home: None, limits: &fixture.limits };
        let engine = Engine::default();
        let year = Query { range: "year", account: None, project: None, date: None };
        assert!(engine.report(&input, &year, now_ist(NOW)).is_err());
        let bad_date = Query { range: "day", account: None, project: None, date: Some("15/10/2026") };
        assert!(engine.report(&input, &bad_date, now_ist(NOW)).is_err());
        let empty_date = Query { range: "day", account: None, project: None, date: Some("") };
        assert!(engine.report(&input, &empty_date, now_ist(NOW)).is_ok());
    }

    #[test]
    fn filters_scope_rows_and_totals_but_keep_the_choices() {
        let dir = TempDir::new();
        dir.write("a/projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/work/alpha", 1_000_000, 0, 0, 0),
            claude_line("r2", "s2", "2026-10-15T01:00:00Z", "claude-opus-5", "/work/beta", 2_000_000, 0, 0, 0),
        ]));
        dir.write("b/projects/-p/a.jsonl", &lines(&[claude_line("r1", "s3", "2026-10-15T01:00:00Z", "claude-opus-5", "/work/gamma", 4_000_000, 0, 0, 0)]));
        let sources = [
            source("claude", Format::Claude, dir.0.join("a/projects")),
            source("claude-work", Format::Claude, dir.0.join("b/projects")),
        ];
        let fixture = Fixture::new();
        let engine = Engine::default();

        let all = fixture.report(&engine, &sources, "day", None, None, None, NOW);
        assert_eq!(all.rows.len(), 3);
        assert_eq!(all.projects.len(), 3);
        assert_eq!(all.accounts.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["claude", "claude-work"]);
        assert!(close(all.total_cost_inr.unwrap(), 35.0 * 84.0));

        let empty_is_all = fixture.report(&engine, &sources, "day", Some(""), Some(""), None, NOW);
        assert_eq!(empty_is_all.rows, all.rows);

        let work = fixture.report(&engine, &sources, "day", Some("claude-work"), None, None, NOW);
        assert_eq!(work.rows.len(), 1);
        assert_eq!(work.projects.len(), 1);
        assert_eq!(work.accounts.len(), 2);
        assert!(close(work.total_cost_inr.unwrap(), 20.0 * 84.0));

        let beta = all.projects.iter().find(|p| p.name == "beta").unwrap().id.clone();
        let one = fixture.report(&engine, &sources, "day", None, Some(&beta), None, NOW);
        assert_eq!(one.rows.len(), 1);
        assert_eq!(one.rows[0].project_name, "beta");
        assert_eq!(one.projects.len(), 3);
        assert!(close(one.total_cost_inr.unwrap(), 10.0 * 84.0));

        let nothing = fixture.report(&engine, &sources, "day", Some("nobody"), None, None, NOW);
        assert!(nothing.rows.is_empty());
        assert_eq!(nothing.coverage, Coverage::None);
    }

    #[test]
    fn plan_share_splits_the_month_and_does_not_move_with_a_filter_or_range() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-10-02T01:00:00Z", "claude-opus-5", "/work/alpha", 3000, 0, 0, 0),
            claude_line("r2", "s2", "2026-10-15T01:00:00Z", "claude-opus-5", "/work/beta", 1000, 0, 0, 0),
        ]));
        let sources = [source("claude", Format::Claude, dir.0.join("projects"))];
        let mut fixture = Fixture::new();
        fixture.settings.accounts.insert(
            "claude".into(),
            AccountSettings { billing: Billing::Subscription, monthly_paid_inr: Some(1000.0), plan_tier: Some("default_claude_max_5x".into()) },
        );
        let engine = Engine::default();

        let month = fixture.report(&engine, &sources, "month", None, None, None, NOW);
        assert!(close(month.plan_share.amount_inr, 1000.0));
        assert_eq!(month.plan_share.coverage, Coverage::Complete);
        assert_eq!(month.budgets[0].monthly_source, "you_entered");
        assert_eq!(month.budgets[0].allocation, "month_token_share");
        assert_eq!(month.budgets[0].plan_name.as_deref(), Some("Max 5x"));

        let beta = month.projects.iter().find(|p| p.name == "beta").unwrap().id.clone();
        let filtered = fixture.report(&engine, &sources, "month", None, Some(&beta), None, NOW);
        assert!(close(filtered.plan_share.amount_inr, 250.0));

        // The day holds only one quarter of the month's weight, whatever the range shown.
        let day = fixture.report(&engine, &sources, "day", None, None, None, NOW);
        assert!(close(day.rows[0].plan_share_inr.unwrap(), 250.0));
        assert_eq!(day.rows[0].estimated_spend_inr, day.rows[0].plan_share_inr);
        // API-equivalent is a separate number, not spend.
        assert!(close(day.rows[0].cost_inr.unwrap(), 1000.0 * 5.0 / 1e6 * 84.0));
    }

    #[test]
    fn a_week_across_two_months_splits_each_by_its_own_month() {
        let dir = TempDir::new();
        // 2026-10-01 is a Thursday: its week starts on 28 September.
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-09-05T01:00:00Z", "claude-opus-5", "/p", 1000, 0, 0, 0),
            claude_line("r2", "s1", "2026-09-29T01:00:00Z", "claude-opus-5", "/p", 1000, 0, 0, 0),
            claude_line("r3", "s1", "2026-10-01T01:00:00Z", "claude-opus-5", "/p", 1000, 0, 0, 0),
        ]));
        let sources = [source("claude", Format::Claude, dir.0.join("projects"))];
        let mut fixture = Fixture::new();
        fixture.settings.accounts.insert("claude".into(), AccountSettings { monthly_paid_inr: Some(100.0), ..AccountSettings::default() });
        let report = fixture.report(&Engine::default(), &sources, "week", None, None, Some("2026-10-01"), NOW);
        assert_eq!(report.from, "2026-09-28");
        assert!(close(report.rows[0].plan_share_inr.unwrap(), 50.0));
        assert!(close(report.rows[1].plan_share_inr.unwrap(), 100.0));
        assert!(close(report.plan_share.amount_inr, 150.0));
    }

    #[test]
    fn plan_amounts_are_honest_about_where_they_came_from() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 1000, 0, 0, 0)]));
        let sources = [source("claude", Format::Claude, dir.0.join("projects"))];
        let run = |account: AccountSettings| {
            let mut fixture = Fixture::new();
            fixture.settings.accounts.insert("claude".into(), account);
            fixture.month(&sources)
        };

        // No plan chosen: unknown, never free. The API-equivalent is still there.
        let unknown = Fixture::new().month(&sources);
        assert_eq!(unknown.budgets[0].monthly_inr, None);
        assert_eq!(unknown.budgets[0].monthly_source, "unknown");
        assert_eq!(unknown.rows[0].plan_share_inr, None);
        assert_eq!(unknown.rows[0].estimated_spend_inr, None);
        assert_eq!(unknown.plan_share.coverage, Coverage::Unavailable);
        assert!(unknown.rows[0].cost_inr.is_some());
        assert!(!unknown.partial);

        // A list price uses the labelled rate.
        let list = run(AccountSettings { plan_tier: Some("default_claude_pro".into()), ..AccountSettings::default() });
        assert!(close(list.budgets[0].monthly_inr.unwrap(), 20.0 * 84.0));
        assert_eq!(list.budgets[0].monthly_source, "plan_list_price");
        assert!(close(list.plan_share.amount_inr, 20.0 * 84.0));

        // A free plan is a real zero.
        let free = run(AccountSettings { plan_tier: Some("default_claude_free".into()), ..AccountSettings::default() });
        assert_eq!(free.budgets[0].monthly_inr, Some(0.0));
        assert_eq!(free.plan_share.coverage, Coverage::Complete);
        assert_eq!(free.plan_share.amount_inr, 0.0);

        // A plan with no published price stays unknown.
        let enterprise = run(AccountSettings { plan_tier: Some("default_claude_enterprise".into()), ..AccountSettings::default() });
        assert_eq!(enterprise.budgets[0].monthly_inr, None);
        assert_eq!(enterprise.budgets[0].plan_name.as_deref(), Some("Enterprise"));

        // What the owner typed beats the list price, and zero is an answer.
        let paid_by_someone = run(AccountSettings { monthly_paid_inr: Some(0.0), plan_tier: Some("default_claude_pro".into()), ..AccountSettings::default() });
        assert_eq!(paid_by_someone.budgets[0].monthly_inr, Some(0.0));
        assert_eq!(paid_by_someone.budgets[0].monthly_source, "you_entered");

        // API billing: the list price is the spend, and there is no plan to split.
        let api = run(AccountSettings { billing: Billing::Api, ..AccountSettings::default() });
        assert_eq!(api.rows[0].plan_share_inr, None);
        assert_eq!(api.rows[0].estimated_spend_inr, api.rows[0].cost_inr);
        assert_eq!(api.plan_share.coverage, Coverage::None);
        assert_eq!(api.api_key_usage.coverage, Coverage::Complete);
        assert_eq!(api.budgets[0].allocation, "none");
    }

    #[test]
    fn broken_lines_are_counted_and_never_become_usage() {
        let dir = TempDir::new();
        let mut text = String::new();
        text.push_str(&user_line("nothing to see"));
        text.push('\n');
        text.push_str(r#"{"type":"assistant","broken"#);
        text.push('\n');
        // Valid JSON missing fields a turn must have.
        text.push_str(r#"{"type":"assistant","message":{"model":"claude-opus-5"}}"#);
        text.push('\n');
        text.push_str(&serde_json::json!({"type":"assistant","cwd":"/p","sessionId":"s","timestamp":"not a date","message":{"model":"claude-opus-5","usage":{"input_tokens":1,"output_tokens":1}}}).to_string());
        text.push('\n');
        text.push_str(&serde_json::json!({"type":"assistant","cwd":"/p","sessionId":"s","timestamp":"2026-10-15T01:00:00Z","message":{"model":"claude-opus-5","usage":{"input_tokens":-5,"output_tokens":1}}}).to_string());
        text.push('\n');
        text.push_str(&claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 10, 1, 0, 0));
        text.push_str("\r\n");
        let mut bytes = text.into_bytes();
        // Bytes that are not text at all, on a line that looks like a turn.
        bytes.extend_from_slice(b"\xFF\xFE\"assistant\"\n");
        // A last line with no newline is still being written.
        bytes.extend_from_slice(claude_line("r2", "s1", "2026-10-15T02:00:00Z", "claude-opus-5", "/p", 1000, 1, 0, 0).as_bytes());
        let path = dir.write("projects/-p/a.jsonl", "");
        std::fs::write(&path, bytes).unwrap();

        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert_eq!(report.rows.len(), 1);
        assert_eq!(report.rows[0].input_tokens, 10);
        assert_eq!(report.scan.lines_skipped, 5);
        assert!(report.partial);
        assert_eq!(report.coverage, Coverage::Complete);
        assert!(report.note.contains("Transcript lines that could not be read: 5."));
    }

    #[test]
    fn malformed_codex_ticks_are_counted() {
        let dir = TempDir::new();
        dir.write("sessions/a.jsonl", &lines(&[
            codex_context("gpt-5", "/p"),
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":"many","output_tokens":1}}},"timestamp":"2026-10-15T02:00:00Z"}"#.to_string(),
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1,"output_tokens":1}}}}"#.to_string(),
            r#"{"type":"event_msg","token_count broken"#.to_string(),
            codex_tick("2026-10-15T02:03:00Z", Some(counters(5, 0, 1)), None),
        ]));
        let report = Fixture::new().month(&[source("codex", Format::Codex, dir.0.join("sessions"))]);
        assert_eq!(report.scan.lines_skipped, 3);
        assert_eq!(report.rows[0].input_tokens, 5);
    }

    #[test]
    fn limits_are_reported_not_silent() {
        let dir = TempDir::new();
        let long = format!("{{\"type\":\"assistant\",\"pad\":\"{}\"}}", "x".repeat(2000));
        dir.write("projects/-p/lines.jsonl", &lines(&[
            long,
            claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 10, 1, 0, 0),
        ]));
        let mut fixture = Fixture::new();
        fixture.limits.max_line_bytes = 1000;
        let sources = [source("claude", Format::Claude, dir.0.join("projects"))];
        let report = fixture.month(&sources);
        assert_eq!(report.scan.lines_skipped, 1);
        assert_eq!(report.rows[0].input_tokens, 10);
        assert!(report.partial);

        let mut fixture = Fixture::new();
        fixture.limits.max_file_bytes = 10;
        let report = fixture.month(&sources);
        assert_eq!(report.scan.files_skipped, 1);
        assert!(report.rows.is_empty());
        assert!(report.partial);
        assert_eq!(report.coverage, Coverage::None);
        assert!(report.note.contains("skipped"));

        dir.write("projects/-p/second.jsonl", &lines(&[claude_line("r9", "s9", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 10, 1, 0, 0)]));
        let mut fixture = Fixture::new();
        fixture.limits.max_files = 1;
        let report = fixture.month(&sources);
        assert_eq!(report.scan.files_read, 1);
        assert_eq!(report.scan.files_skipped, 1);

        let mut fixture = Fixture::new();
        fixture.limits.max_total_bytes = 1;
        let report = fixture.month(&sources);
        assert_eq!(report.scan.files_read, 0);
        assert_eq!(report.scan.files_skipped, 2);

        let mut fixture = Fixture::new();
        fixture.limits.max_entries = 1;
        assert!(fixture.month(&sources).scan.files_skipped >= 1);
    }

    #[test]
    fn line_reading_splits_on_newlines_whatever_the_buffer_size() {
        let text = b"one\ntwo-longer\n\nthree-is-too-long-to-keep\nfour\nunfinished";
        for capacity in [1, 2, 3, 7, 64] {
            let mut seen: Vec<String> = Vec::new();
            let mut long = 0;
            for_each_line(BufReader::with_capacity(capacity, &text[..]), 12, &mut long, |line| {
                seen.push(String::from_utf8_lossy(line).into_owned());
            })
            .unwrap();
            assert_eq!(seen, ["one", "two-longer", "", "four"], "capacity {capacity}");
            assert_eq!(long, 1, "capacity {capacity}");
        }
    }

    #[test]
    fn old_files_are_not_read_and_missing_roots_are_fine() {
        let dir = TempDir::new();
        let sources = [source("claude", Format::Claude, dir.0.join("nowhere"))];
        let report = Fixture::new().month(&sources);
        assert_eq!(report.coverage, Coverage::None);
        assert_eq!(report.accounts.len(), 1);
        assert!(!report.partial);

        // A file last written before the scan window cannot hold a turn inside it.
        dir.write("projects/-p/a.jsonl", &lines(&[claude_line("r1", "s1", "2020-01-01T01:00:00Z", "claude-opus-5", "/p", 1, 1, 0, 0)]));
        let sources = [source("claude", Format::Claude, dir.0.join("projects"))];
        let far_future = Fixture::new().report(&Engine::default(), &sources, "day", None, None, None, "2099-01-15T12:00:00+05:30");
        assert_eq!(far_future.scan.files_read, 0);
    }

    #[test]
    fn hidden_files_and_other_extensions_are_ignored() {
        let dir = TempDir::new();
        let line = lines(&[claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 1, 1, 0, 0)]);
        dir.write("projects/.hidden/a.jsonl", &line);
        dir.write("projects/-p/.b.jsonl", &line);
        dir.write("projects/-p/c.json", &line);
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert!(report.rows.is_empty());
        assert_eq!(report.scan.files_read, 0);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_not_followed() {
        let dir = TempDir::new();
        let outside = dir.write("outside/a.jsonl", &lines(&[claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 1, 1, 0, 0)]));
        std::fs::create_dir_all(dir.0.join("projects")).unwrap();
        std::os::unix::fs::symlink(outside.parent().unwrap(), dir.0.join("projects/linked")).unwrap();
        std::os::unix::fs::symlink(&outside, dir.0.join("projects/link.jsonl")).unwrap();
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert!(report.rows.is_empty());
    }

    #[test]
    fn the_cache_reads_a_changed_file_again_and_keeps_an_unchanged_one() {
        let dir = TempDir::new();
        let first = lines(&[claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 10, 1, 0, 0)]);
        let path = dir.write("projects/-p/a.jsonl", &first);
        let sources = [source("claude", Format::Claude, dir.0.join("projects"))];
        let fixture = Fixture::new();
        let engine = Engine::default();
        assert_eq!(fixture.report(&engine, &sources, "day", None, None, None, NOW).rows[0].input_tokens, 10);
        let cached = {
            let state = engine.state.lock().unwrap();
            state.files.get(&path).unwrap().parse.clone()
        };
        assert_eq!(fixture.report(&engine, &sources, "day", None, None, None, NOW).rows[0].input_tokens, 10);
        {
            let state = engine.state.lock().unwrap();
            assert!(Arc::ptr_eq(&cached, &state.files.get(&path).unwrap().parse));
        }
        let mut more = first.clone();
        more.push_str(&lines(&[claude_line("r2", "s1", "2026-10-15T02:00:00Z", "claude-opus-5", "/p", 5, 1, 0, 0)]));
        std::fs::write(&path, more).unwrap();
        assert_eq!(fixture.report(&engine, &sources, "day", None, None, None, NOW).rows[0].input_tokens, 15);
    }

    #[test]
    fn the_report_carries_no_prompts_answers_or_paths() {
        let dir = TempDir::new();
        let home = dir.0.join("users").join("private-user");
        let repo = home.join("deep").join("client-project");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let cwd = repo.to_string_lossy().into_owned();
        dir.write("projects/-p/a.jsonl", &lines(&[
            user_line("SECRET-PROMPT-TEXT sk-ant-SECRET-KEY assistant"),
            claude_line("r1", "s1-session-long", "2026-10-15T01:00:00Z", "claude-opus-5", &cwd, 10, 1, 0, 0),
        ]));
        dir.write("sessions/a.jsonl", &lines(&[
            codex_meta("codex-session", &cwd),
            codex_context("gpt-5", &cwd),
            serde_json::json!({"timestamp":"2026-10-15T01:00:00Z","type":"response_item","payload":{"type":"message","content":"SECRET-PROMPT-TEXT token_count"}}).to_string(),
            codex_tick("2026-10-15T02:00:00Z", Some(counters(5, 0, 1)), None),
        ]));
        let sources = [
            source("claude", Format::Claude, dir.0.join("projects")),
            source("codex", Format::Codex, dir.0.join("sessions")),
        ];
        let mut fixture = Fixture::new();
        fixture.home = Some(home.to_string_lossy().into_owned());
        let report = fixture.month(&sources);
        assert_eq!(report.rows.len(), 2);
        assert_eq!(report.scan.lines_skipped, 0);
        assert_eq!(report.projects.len(), 1);
        assert_eq!(report.projects[0].name, "client-project");
        let text = serde_json::to_string(&report).unwrap();
        for forbidden in ["SECRET", "sk-ant", "private-user", "deep", dir.0.to_string_lossy().as_ref()] {
            assert!(!text.contains(forbidden), "report leaked {forbidden}");
        }
        assert_eq!(report.sessions[0].id.chars().count(), 8);
    }

    #[test]
    fn project_names_stay_short_and_never_carry_the_home_folder() {
        let home = Some("C:\\Users\\pandit");
        assert_eq!(project_name("C:\\Users\\pandit", home), "Home");
        assert_eq!(project_name("c:/users/pandit/", home), "Home");
        assert_eq!(project_name("C:\\Users\\pandit\\src", home), "src");
        assert_eq!(project_name("C:\\work\\matra\\native", home), "native");
        assert_eq!(project_name("C:\\work\\nodes\\src", home), "nodes/src");
        assert_eq!(project_name("D:\\src", home), "src");
        assert_eq!(project_name("/home/u/work/matra", None), "matra");
        assert_eq!(project_name("", home), "Unknown project");
        assert_eq!(project_name("/", None), "Unknown project");
        assert!(project_name(&format!("/w/{}", "n".repeat(500)), None).chars().count() <= MAX_NAME_CHARS);
    }

    #[test]
    fn one_project_whichever_way_its_path_is_spelled() {
        assert_eq!(project_id("C:\\Work\\Matra"), project_id("c:/work/matra/"));
        assert_ne!(project_id("/work/a"), project_id("/work/b"));
        assert_ne!(project_id("/work/A"), project_id("/work/a"));
        assert_eq!(project_id(""), UNKNOWN_PROJECT_ID);
        assert!(!project_id("/secret/path").contains("secret"));
    }

    #[test]
    fn same_named_folders_stay_apart() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/one/matra", 1, 1, 0, 0),
            claude_line("r2", "s2", "2026-10-15T01:00:00Z", "claude-opus-5", "/two/matra", 1, 1, 0, 0),
        ]));
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert_eq!(report.projects.len(), 2);
        assert_ne!(report.projects[0].name, report.projects[1].name);
        assert!(report.projects.iter().all(|p| p.name.starts_with("matra (")));
    }

    #[test]
    fn subfolders_of_one_repo_are_one_project() {
        let dir = TempDir::new();
        let repo = dir.0.join("repo-root");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("a/b")).unwrap();
        let (top, deep) = (repo.to_string_lossy().into_owned(), repo.join("a/b").to_string_lossy().into_owned());
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", &top, 1, 1, 0, 0),
            claude_line("r2", "s2", "2026-10-15T01:00:00Z", "claude-opus-5", &deep, 1, 1, 0, 0),
        ]));
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert_eq!(report.projects.len(), 1);
        assert_eq!(report.projects[0].name, "repo-root");
    }

    #[test]
    fn a_deleted_projects_folder_name_recovers_its_root() {
        assert_eq!(ProjectResolver::root_of("/gone/my-app/src/deep", "-gone-my-app"), "/gone/my-app");
        assert_eq!(ProjectResolver::root_of("C:\\gone\\my-app\\src", "C--gone-my-app"), "C:\\gone\\my-app");
        assert_eq!(ProjectResolver::root_of("/gone/other", "-unrelated"), "/gone/other");
        assert_eq!(ProjectResolver::root_of("", "-x"), "");
        // A network path is never touched, only named.
        assert_eq!(ProjectResolver::root_of("\\\\server\\share\\w", ""), "\\\\server\\share\\w");
        assert_eq!(ProjectResolver::root_of("//server/share/w", ""), "//server/share/w");
    }

    #[test]
    fn a_subagent_transcript_counts_under_its_sessions_project_folder() {
        let dir = TempDir::new();
        dir.write("projects/-gone-mytool/sess/subagents/agent-1.jsonl", &lines(&[
            claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/gone/mytool/src/deep", 1, 1, 0, 0),
        ]));
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert_eq!(report.projects[0].name, "mytool");
    }

    #[test]
    fn sessions_list_metadata_newest_first_and_cap_duration_by_day() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[
            claude_line("r1", "aaaaaaaa-1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 1, 1, 0, 0),
            claude_line("r2", "aaaaaaaa-1", "2026-10-15T02:00:00Z", "claude-haiku-4", "/p", 1, 1, 0, 0),
            claude_line("r3", "bbbbbbbb-2", "2026-10-15T03:00:00Z", "claude-opus-5", "/p", 1, 1, 0, 0),
            // The same session the next local day: two stretches, not a day and a half of work.
            claude_line("r4", "aaaaaaaa-1", "2026-10-16T01:00:00Z", "claude-opus-5", "/p", 1, 1, 0, 0),
        ]));
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        assert_eq!(report.sessions.len(), 2);
        let first = &report.sessions[0];
        assert_eq!(first.id, "aaaaaaaa");
        assert_eq!(first.models, ["claude-haiku-4", "claude-opus-5"]);
        assert_eq!(first.turns, 3);
        assert_eq!(first.tokens, 6);
        assert_eq!(first.branch.as_deref(), Some("main"));
        assert_eq!(first.duration_seconds, 3600 + MIN_SLICE_SECONDS);
        assert_eq!(report.sessions[1].duration_seconds, MIN_SLICE_SECONDS);
        assert!(!report.sessions_truncated);
    }

    #[test]
    fn settings_default_to_the_contract_and_say_the_rate_is_an_estimate() {
        let dir = TempDir::new();
        let settings = load_settings(&dir.0);
        assert_eq!(settings, CostSettings::default());
        assert_eq!(settings.usd_to_inr, 84.0);
        assert_eq!(settings.currency, "INR");
        assert!(settings.custom_prices.is_empty());
        let report = Fixture::new().month(&[]);
        assert_eq!(report.rate_label, RATE_LABEL);
        assert_eq!(report.usd_to_inr, 84.0);
        assert!(report.note.contains("not a live exchange rate"));
        // Nothing was written just by reading.
        assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 0);
    }

    #[test]
    fn settings_round_trip_atomically_and_reject_bad_values() {
        let dir = TempDir::new();
        let costs = dir.0.join("costs");
        let mut settings = CostSettings { usd_to_inr: 90.5, ..CostSettings::default() };
        settings.custom_prices = vec![
            ModelPrice { model: " gpt-x ".into(), input: 1.0, output: 2.0, cache_read: Some(0.1), cache_write: None },
            ModelPrice { model: "GPT.X".into(), input: 3.0, output: 4.0, cache_read: None, cache_write: None },
        ];
        settings.accounts.insert("claude".into(), AccountSettings { billing: Billing::Api, monthly_paid_inr: Some(0.0), plan_tier: Some(" pro ".into()) });
        let saved = save_settings(&costs, settings).unwrap();
        assert_eq!(saved.custom_prices.len(), 1);
        assert_eq!(saved.custom_prices[0].input, 3.0);
        assert_eq!(saved.accounts["claude"].plan_tier.as_deref(), Some("pro"));
        assert_eq!(load_settings(&costs), saved);
        let names: Vec<String> = std::fs::read_dir(&costs).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["settings.json"]);

        let refused = |settings: CostSettings| save_settings(&costs, settings).is_err();
        let price = |model: &str, input: f64, cache_read: Option<f64>| CostSettings {
            custom_prices: vec![ModelPrice { model: model.into(), input, output: 0.0, cache_read, cache_write: None }],
            ..CostSettings::default()
        };
        let account = |id: &str, paid: Option<f64>| {
            let mut settings = CostSettings::default();
            settings.accounts.insert(id.into(), AccountSettings { monthly_paid_inr: paid, ..AccountSettings::default() });
            settings
        };
        for rate in [0.0, -1.0, f64::NAN, f64::INFINITY, MAX_RATE * 2.0] {
            assert!(refused(CostSettings { usd_to_inr: rate, ..CostSettings::default() }), "rate {rate}");
        }
        assert!(refused(CostSettings { currency: "USD".into(), ..CostSettings::default() }));
        assert!(refused(price("m", -1.0, None)));
        assert!(refused(price("", 1.0, None)));
        assert!(refused(price("with space", 1.0, None)));
        assert!(refused(price("m", 1.0, Some(-1.0))));
        assert!(refused(account("a", Some(-1.0))));
        assert!(refused(account("", None)));
        // A refused save left the good file alone.
        assert_eq!(load_settings(&costs), saved);
    }

    #[test]
    fn settings_accept_partial_objects_and_refuse_a_bad_billing_value() {
        let partial: CostSettings = serde_json::from_str(r#"{"usd_to_inr": 85}"#).unwrap();
        assert_eq!(partial.currency, "INR");
        assert_eq!(partial.usd_to_inr, 85.0);
        let camel: CostSettings = serde_json::from_str(r#"{"custom_prices":[{"model":"m","input":1,"output":2,"cacheRead":0.5,"cacheWrite":3}]}"#).unwrap();
        assert_eq!(camel.custom_prices[0].cache_read, Some(0.5));
        assert_eq!(camel.custom_prices[0].cache_write, Some(3.0));
        assert!(serde_json::from_str::<CostSettings>(r#"{"accounts":{"a":{"billing":"free"}}}"#).is_err());
    }

    #[test]
    fn a_damaged_settings_file_reads_as_defaults_and_is_left_for_the_owner() {
        let dir = TempDir::new();
        let path = dir.write("settings.json", "{ not json");
        assert_eq!(load_settings(&dir.0), CostSettings::default());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
        dir.write("settings.json", r#"{"usd_to_inr": -5}"#);
        assert_eq!(load_settings(&dir.0), CostSettings::default());
        dir.write("settings.json", &" ".repeat(MAX_SETTINGS_BYTES as usize + 1));
        assert_eq!(load_settings(&dir.0), CostSettings::default());
    }

    #[cfg(unix)]
    #[test]
    fn saving_refuses_to_write_through_a_symlink() {
        let dir = TempDir::new();
        let elsewhere = dir.write("elsewhere/settings.json", "{}");
        let costs = dir.0.join("costs");
        std::fs::create_dir_all(&costs).unwrap();
        std::os::unix::fs::symlink(&elsewhere, costs.join("settings.json")).unwrap();
        assert!(save_settings(&costs, CostSettings::default()).is_err());
        assert_eq!(std::fs::read_to_string(&elsewhere).unwrap(), "{}");
        assert_eq!(std::fs::read_dir(&costs).unwrap().count(), 1);

        let linked = dir.0.join("linked");
        std::os::unix::fs::symlink(elsewhere.parent().unwrap(), &linked).unwrap();
        assert!(save_settings(&linked, CostSettings::default()).is_err());
    }

    #[test]
    fn bundled_prices_and_plans_match_the_mac_defaults() {
        let prices = prices_from_json(PRICES_DEFAULT.as_bytes());
        assert_eq!(prices.len(), 6);
        let opus = prices.iter().find(|p| p.model == "claude-opus-5").unwrap();
        assert_eq!((opus.input, opus.output, opus.cache_read, opus.cache_write), (5.0, 25.0, Some(0.5), Some(6.25)));
        let plans = plans_from_json(PLANS_DEFAULT.as_bytes()).unwrap();
        assert_eq!(plans.len(), 13);
        assert_eq!(plans["default_claude_max_5x"].usd, Some(100.0));
        assert_eq!(plans["pro"].usd, Some(200.0));
        assert_eq!(plans["business"].credit_usd, Some(1.0));
        assert_eq!(plans["edu"].usd, None);
        assert_eq!(plan_monthly_inr(&plans["plus"], 84.0), Some(1680.0));
        assert_eq!(plan_monthly_inr(&plans["free"], 84.0), Some(0.0));
        assert_eq!(plan_monthly_inr(&plans["edu"], 84.0), None);
        let rupees = PlanInfo { tier: "x".into(), name: "x".into(), usd: Some(1.0), inr: Some(500.0), credit_usd: None };
        assert_eq!(plan_monthly_inr(&rupees, 84.0), Some(500.0));
    }

    #[test]
    fn an_edited_plan_catalog_is_used_and_a_damaged_one_falls_back() {
        let dir = TempDir::new();
        assert_eq!(load_plans(&dir.0).len(), 13);
        dir.write("plans.json", r#"{"plans":{"mine":{"name":"Mine","prices":{"INR":999}}}}"#);
        let plans = load_plans(&dir.0);
        assert_eq!(plans.len(), 1);
        assert_eq!(plans["mine"].inr, Some(999.0));
        dir.write("plans.json", "not json");
        assert_eq!(load_plans(&dir.0).len(), 13);
    }

    #[test]
    fn openrouter_entries_are_kept_only_when_they_are_real_prices() {
        let raw = br#"{"data":[
            {"id":"anthropic/claude-fable-5.1","pricing":{"prompt":"0.00001","completion":"0.00005","input_cache_read":"0.000001","input_cache_write":"0.0000125"}},
            {"id":"openai/gpt-6.1","pricing":{"prompt":"0.00001","completion":"0.00005"}},
            {"id":"openai/free:free","pricing":{"prompt":"0","completion":"0"}},
            {"id":"openai/zero","pricing":{"prompt":"0","completion":"0"}},
            {"id":"google/gemini","pricing":{"prompt":"0.1","completion":"0.1"}},
            {"id":"anthropic/no-price","pricing":{"prompt":"abc","completion":"1"}},
            {"id":"anthropic/no-pricing"}
        ]}"#;
        let prices = parse_openrouter(raw);
        assert_eq!(prices.len(), 2);
        assert_eq!(prices[0].model, "claude-fable-5-1");
        assert!(close(prices[0].input, 10.0));
        assert!(close(prices[0].cache_read.unwrap(), 1.0));
        assert_eq!(prices[1].model, "gpt-6-1");
        assert_eq!(prices[1].cache_read, None);
        assert!(parse_openrouter(b"not json").is_empty());
        assert!(parse_openrouter(br#"{"data":"nope"}"#).is_empty());
    }

    #[test]
    fn accounts_are_found_from_their_folders() {
        let dir = TempDir::new();
        dir.write(".codex-work/auth.json", "{}");
        dir.write(".codex-empty/readme.txt", "nothing");
        std::fs::create_dir_all(dir.0.join(".codex-sessions-only/sessions")).unwrap();
        dir.write(".codex-file", "not a folder");
        let claude = [dir.0.join(".claude"), dir.0.join(".claude-work")];
        let sources = discover_sources(Some(&dir.0), &claude);
        let ids: Vec<&str> = sources.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["claude", "claude-work", "codex", "codex-sessions-only", "codex-work"]);
        assert_eq!(sources[1].name, "Claude (work)");
        assert_eq!(sources[4].name, "Codex (work)");
        assert_eq!(sources[0].roots, [dir.0.join(".claude/projects")]);
        assert_eq!(sources[2].roots, [dir.0.join(".codex/sessions"), dir.0.join(".codex/archived_sessions")]);
        assert_eq!(discover_sources(None, &[]).len(), 0);
    }

    #[test]
    fn an_archived_rollout_counts_once_alongside_its_live_copy() {
        let dir = TempDir::new();
        let text = lines(&[
            codex_meta("sess-1", "/p"),
            codex_context("gpt-5", "/p"),
            codex_tick("2026-10-15T02:00:00.100Z", Some(counters(100, 0, 10)), Some(counters(100, 0, 10))),
        ]);
        dir.write("sessions/a.jsonl", &text);
        dir.write("archived_sessions/a.jsonl", &text);
        let sources = [AccountSource {
            id: "codex".into(),
            name: "Codex".into(),
            format: Format::Codex,
            roots: vec![dir.0.join("sessions"), dir.0.join("archived_sessions")],
        }];
        let report = Fixture::new().month(&sources);
        assert_eq!(report.scan.files_read, 2);
        assert_eq!(report.rows[0].turns, 1);
        assert_eq!(report.rows[0].input_tokens, 100);
    }

    #[test]
    fn the_report_serialises_to_the_published_shape() {
        let dir = TempDir::new();
        dir.write("projects/-p/a.jsonl", &lines(&[claude_line("r1", "s1", "2026-10-15T01:00:00Z", "claude-opus-5", "/p", 1, 1, 0, 0)]));
        let report = Fixture::new().month(&[source("claude", Format::Claude, dir.0.join("projects"))]);
        let json = serde_json::to_value(&report).unwrap();
        for key in ["range", "accounts", "projects", "rows", "total_cost_inr", "unpriced_turns", "partial", "note", "coverage", "budgets", "sessions", "scan", "rate_label"] {
            assert!(json.get(key).is_some(), "missing {key}");
        }
        let row = &json["rows"][0];
        for key in ["day", "account", "project", "model", "input_tokens", "output_tokens", "cached_tokens", "cost_inr"] {
            assert!(row.get(key).is_some(), "missing row {key}");
        }
        assert_eq!(json["coverage"], "complete");
        assert_eq!(json["budgets"][0]["billing"], "subscription");
    }
}
