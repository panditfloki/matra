//! Provider registry, settings and polling, with every path and environment read injected so tests never
//! touch a real profile. The Tauri layer in `providers_plus.rs` only forwards to this.
//!
//! User-entered keys are bound to the destination origin in Windows Credential Manager.
use super::bridge;
use super::custom::{self, CustomEndpoint, Detection, Mapping, Preset, TestResult};
use super::deepseek;
use super::grokbot;
use super::higgsfield;
use super::local::{self, OllamaCloud, OmlxSession};
use super::model::{now_ms, Failure, ParitySnapshot, Reading, LOCAL, USAGE};
use super::net;
use super::settings::{self, Settings};
use crate::usage::UsageSnapshot;
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub const FIXED_IDS: [&str; 7] = ["grokbot", "higgsfield", "deepseek", "ollama", "lmstudio", "omlx", "llamacpp"];
const MAX_CONCURRENT: usize = 8;

pub const SECRETS_AVAILABLE: bool = super::vault::AVAILABLE;
const NO_KEY_STORAGE: &str = super::vault::UNAVAILABLE;
use super::vault::{read as stored_key, write as store_key, remove as forget_key};

fn custom_key_id(endpoint: &CustomEndpoint) -> String {
    format!("custom:{}@{:?}", endpoint.id, net::origin(&endpoint.base_url))
}

// MARK: Environment

pub type Vars = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

#[derive(Clone)]
pub struct Urls {
    pub ollama_cloud: String,
    pub deepseek: String,
    pub higgsfield: String,
}

impl Default for Urls {
    fn default() -> Self {
        Urls {
            ollama_cloud: local::OLLAMA_CLOUD.into(),
            deepseek: deepseek::BALANCE_URL.into(),
            higgsfield: higgsfield::GATEWAY.into(),
        }
    }
}

#[derive(Clone)]
pub struct Env {
    pub settings_dir: PathBuf,
    pub home: Option<PathBuf>,
    pub appdata: Option<PathBuf>,
    pub local_appdata: Option<PathBuf>,
    pub vars: Vars,
    pub path_var: Option<OsString>,
    pub urls: Urls,
}

impl Env {
    fn var(&self, name: &str) -> Option<String> {
        (self.vars)(name).and_then(|v| net::clean_secret(&v))
    }
}

fn fixed_name(id: &str) -> &'static str {
    match id {
        "grokbot" => "Grok Bot",
        "higgsfield" => "Higgsfield",
        "deepseek" => "DeepSeek",
        "ollama" => "Ollama",
        "lmstudio" => "LM Studio",
        "omlx" => "oMLX",
        "llamacpp" => "llama.cpp",
        _ => "",
    }
}

fn kind_of(id: &str) -> &'static str {
    if matches!(id, "lmstudio" | "omlx" | "llamacpp" | "ollama") {
        LOCAL
    } else {
        USAGE
    }
}

fn default_endpoint(id: &str) -> Option<&'static str> {
    match id {
        "ollama" => Some(local::OLLAMA_DEFAULT),
        "lmstudio" => Some(local::LMSTUDIO_DEFAULT),
        "omlx" => Some(local::OMLX_DEFAULT),
        "llamacpp" => Some(local::LLAMACPP_DEFAULT),
        _ => None,
    }
}

fn env_names(id: &str) -> &'static [&'static str] {
    match id {
        "higgsfield" => &higgsfield::ENV_KEYS,
        "deepseek" => &[deepseek::ENV_KEY],
        "ollama" => &["OLLAMA_API_KEY"],
        "lmstudio" => &["LM_API_TOKEN"],
        "omlx" => &["OMLX_API_KEY"],
        "llamacpp" => &["LLAMA_API_KEY"],
        _ => &[],
    }
}

fn credential_kind(id: &str) -> &'static str {
    match id {
        "grokbot" => "borrowed",
        "higgsfield" => "borrowed",
        "deepseek" => "required",
        id if id.starts_with("custom:") => "optional",
        _ => "optional",
    }
}

fn secret_label(id: &str) -> Option<&'static str> {
    match id {
        "higgsfield" => Some("Higgsfield API key"),
        "deepseek" => Some("DeepSeek API key"),
        "ollama" => Some("Ollama API key"),
        "lmstudio" => Some("LM Studio API token"),
        "omlx" => Some("oMLX API key"),
        "llamacpp" => Some("llama-server API key"),
        _ => None,
    }
}

fn guidance(id: &str) -> &'static str {
    match id {
        "grokbot" => "Reads the weekly usage Grok Bot keeps on this PC. It only updates while Grok Bot runs.",
        "higgsfield" => "Uses the Higgsfield CLI's sign-in when installed, or a key from HIGGSFIELD_API_KEY.",
        "deepseek" => "Reads your balance with a key from DEEPSEEK_API_KEY. The public API reports no spend or allowance, so there is no percentage.",
        "ollama" => "Reads models loaded on this PC. Cloud usage needs a key from OLLAMA_API_KEY.",
        "lmstudio" => "Reads models loaded on this PC. Start the server in LM Studio's Developer tab.",
        "omlx" => "oMLX runs on a Mac. Enter that Mac's address to monitor it from this PC.",
        "llamacpp" => "Reads llama-server's metrics. Start it with --metrics.",
        _ => "",
    }
}

fn poll_ms(id: &str, settings: &Settings) -> u64 {
    match id {
        "lmstudio" | "omlx" | "llamacpp" | "ollama" => 10_000,
        "grokbot" => 60_000,
        "higgsfield" | "deepseek" => 300_000,
        _ => settings
            .custom
            .iter()
            .find(|e| Some(e.id.as_str()) == id.strip_prefix("custom:"))
            .filter(|e| url::Url::parse(&e.base_url).is_ok_and(|u| net::is_loopback(&u)))
            .map_or(120_000, |_| 30_000),
    }
}

/// The shortest gap a manual refresh or a look at the card may force.
fn refresh_floor_ms(id: &str) -> u64 {
    match id {
        "lmstudio" | "omlx" | "llamacpp" | "ollama" => 5_000,
        _ => 30_000,
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub default_endpoint: Option<String>,
    pub endpoint_scope: String,
    pub credential: String,
    pub accepts_secret: bool,
    pub has_stored_secret: bool,
    pub has_env_secret: bool,
    pub secret_label: Option<String>,
    pub guidance: String,
}

#[derive(Default)]
struct Slot {
    snapshot: ParitySnapshot,
    next: u64,
    last_attempt: u64,
    generation: u64,
    in_flight: bool,
    rate_limits: u32,
}

struct State {
    settings: Settings,
    slots: BTreeMap<String, Slot>,
    backoff: BTreeMap<String, u64>,
    running: usize,
}

pub struct Engine {
    env: Env,
    state: Mutex<State>,
    ollama: Mutex<OllamaCloud>,
    omlx: Mutex<OmlxSession>,
    /// Bumped when a credential or address changes, so a read already under way cannot restore old state.
    epoch: AtomicU64,
    notify: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

fn absent(id: &str, name: &str) -> ParitySnapshot {
    ParitySnapshot {
        base: UsageSnapshot {
            status: "absent".into(),
            ..Default::default()
        },
        display_name: name.to_string(),
        kind: kind_of(id).to_string(),
        ..Default::default()
    }
}

fn checking(id: &str, name: &str) -> ParitySnapshot {
    ParitySnapshot {
        base: UsageSnapshot {
            status: "needsAuth".into(),
            note: "Checking...".into(),
            ..Default::default()
        },
        display_name: name.to_string(),
        kind: kind_of(id).to_string(),
        ..Default::default()
    }
}

fn custom_id(id: &str) -> Option<&str> {
    id.strip_prefix("custom:")
}

pub fn is_parity_id(id: &str) -> bool {
    FIXED_IDS.contains(&id) || custom_id(id).is_some_and(custom::valid_id)
}

fn utc_day(now: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now as i64)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

impl Engine {
    pub fn new(env: Env) -> Self {
        let settings = settings::load(&env.settings_dir);
        let mut backoff = settings::load_backoff(&env.settings_dir);
        let now = now_ms();
        backoff.retain(|_, until| *until > now);
        let mut slots = BTreeMap::new();
        for id in FIXED_IDS {
            let enabled = settings.enabled.iter().any(|e| e == id);
            slots.insert(
                id.to_string(),
                Slot {
                    snapshot: if enabled { checking(id, fixed_name(id)) } else { absent(id, fixed_name(id)) },
                    ..Default::default()
                },
            );
        }
        for endpoint in &settings.custom {
            let id = format!("custom:{}", endpoint.id);
            slots.insert(
                id.clone(),
                Slot {
                    snapshot: if endpoint.is_enabled { checking(&id, &endpoint.name) } else { absent(&id, &endpoint.name) },
                    ..Default::default()
                },
            );
        }
        for (id, until) in &backoff {
            if let Some(slot) = slots.get_mut(id) {
                slot.next = *until;
                slot.snapshot.base.backoff_until = *until;
                if slot.snapshot.base.status != "absent" {
                    slot.snapshot.base.status = "backoff".into();
                    slot.snapshot.base.note = "Waiting out a rate limit before retrying.".into();
                }
            }
        }
        Engine {
            env,
            state: Mutex::new(State {
                settings,
                slots,
                backoff,
                running: 0,
            }),
            ollama: Mutex::new(OllamaCloud::default()),
            omlx: Mutex::new(OmlxSession::default()),
            epoch: AtomicU64::new(0),
            notify: Mutex::new(None),
        }
    }

    pub fn set_notify(&self, notify: Arc<dyn Fn() + Send + Sync>) {
        *self.notify.lock().unwrap() = Some(notify);
    }

    fn changed(&self) {
        let notify = self.notify.lock().unwrap().clone();
        if let Some(notify) = notify {
            notify();
        }
    }

    pub fn usage_map(&self) -> BTreeMap<String, ParitySnapshot> {
        let state = self.state.lock().unwrap();
        state.slots.iter().map(|(id, slot)| (id.clone(), slot.snapshot.clone())).collect()
    }

    pub fn snapshot(&self, id: &str) -> UsageSnapshot {
        self.state
            .lock()
            .unwrap()
            .slots
            .get(id)
            .map(|slot| slot.snapshot.base.clone())
            .unwrap_or_else(|| UsageSnapshot {
                status: "absent".into(),
                ..Default::default()
            })
    }

    pub fn enabled_ids(&self) -> Vec<String> {
        let state = self.state.lock().unwrap();
        let mut ids: Vec<String> = state.settings.enabled.clone();
        ids.extend(state.settings.custom.iter().filter(|e| e.is_enabled).map(|e| format!("custom:{}", e.id)));
        ids
    }

    fn resolved_endpoint(&self, id: &str, settings: &Settings) -> Option<String> {
        if let Some(saved) = settings.endpoints.get(id) {
            return Some(saved.clone());
        }
        if id == "lmstudio" {
            if let Some(found) = self.env.home.as_deref().and_then(local::lmstudio_configured) {
                return Some(found);
            }
        }
        default_endpoint(id).map(str::to_owned)
    }

    fn info(&self, id: &str, settings: &Settings) -> Option<ProviderInfo> {
        if let Some(rest) = custom_id(id) {
            let e = settings.custom.iter().find(|e| e.id == rest)?;
            return Some(ProviderInfo {
                id: id.to_string(),
                name: e.name.clone(),
                kind: USAGE.into(),
                enabled: e.is_enabled,
                endpoint: Some(e.base_url.clone()),
                default_endpoint: None,
                endpoint_scope: "network".into(),
                credential: "optional".into(),
                accepts_secret: SECRETS_AVAILABLE,
                has_stored_secret: stored_key(&self.key_id(id, settings)).is_some(),
                has_env_secret: false,
                secret_label: Some("API key".into()),
                guidance: String::new(),
            });
        }
        if !FIXED_IDS.contains(&id) {
            return None;
        }
        let names = env_names(id);
        Some(ProviderInfo {
            id: id.to_string(),
            name: fixed_name(id).to_string(),
            kind: kind_of(id).to_string(),
            enabled: settings.enabled.iter().any(|e| e == id),
            endpoint: self.resolved_endpoint(id, settings),
            default_endpoint: default_endpoint(id).map(str::to_owned),
            endpoint_scope: match id {
                "omlx" => "network",
                "ollama" | "lmstudio" | "llamacpp" => "loopback",
                _ => "none",
            }
            .into(),
            credential: credential_kind(id).into(),
            accepts_secret: SECRETS_AVAILABLE && secret_label(id).is_some(),
            has_stored_secret: stored_key(&self.key_id(id, settings)).is_some(),
            has_env_secret: names.iter().any(|n| self.env.var(n).is_some()),
            secret_label: secret_label(id).map(str::to_owned),
            guidance: guidance(id).to_string(),
        })
    }

    pub fn registry(&self) -> Vec<ProviderInfo> {
        let settings = self.state.lock().unwrap().settings.clone();
        FIXED_IDS.iter().filter_map(|id| self.info(id, &settings)).collect()
    }

    fn persist(&self, state: &mut State, next: Settings) -> Result<(), String> {
        settings::save(&self.env.settings_dir, &next)?;
        state.settings = next;
        Ok(())
    }

    fn reset_slot(state: &mut State, id: &str, on: bool, name: &str) {
        let slot = state.slots.entry(id.to_string()).or_default();
        slot.generation += 1;
        slot.next = 0;
        slot.last_attempt = 0;
        slot.rate_limits = 0;
        let until = state.backoff.get(id).copied().unwrap_or(0);
        slot.snapshot = if on { checking(id, name) } else { absent(id, name) };
        if on && until > now_ms() {
            slot.next = until;
            slot.snapshot.base.status = "backoff".into();
            slot.snapshot.base.backoff_until = until;
            slot.snapshot.base.note = "Waiting out a rate limit before retrying.".into();
        }
    }

    pub fn set_enabled(&self, id: &str, on: bool) -> Result<ProviderInfo, String> {
        if !is_parity_id(id) {
            return Err("Unknown provider.".into());
        }
        let info = {
            let mut state = self.state.lock().unwrap();
            let mut next = state.settings.clone();
            let name;
            if let Some(rest) = custom_id(id) {
                let endpoint = next.custom.iter_mut().find(|e| e.id == rest).ok_or("Unknown provider.")?;
                endpoint.is_enabled = on;
                name = endpoint.name.clone();
            } else {
                next.enabled.retain(|e| e != id);
                if on {
                    next.enabled.push(id.to_string());
                }
                name = fixed_name(id).to_string();
            }
            self.persist(&mut state, next)?;
            Self::reset_slot(&mut state, id, on, &name);
            self.info(id, &state.settings).ok_or("Unknown provider.")?
        };
        self.changed();
        Ok(info)
    }

    pub fn set_endpoint(&self, id: &str, endpoint: Option<&str>) -> Result<ProviderInfo, String> {
        let parsed = match (id, endpoint.map(str::trim).filter(|e| !e.is_empty())) {
            (_, None) if default_endpoint(id).is_some() => None,
            ("omlx", Some(address)) => Some(net::parse_network(address)?),
            ("ollama" | "lmstudio" | "llamacpp", Some(address)) => Some(net::parse_loopback(address)?),
            _ => return Err("This provider has no address to set.".into()),
        };
        let info = {
            let mut state = self.state.lock().unwrap();
            let mut next = state.settings.clone();
            match parsed {
                Some(address) => next.endpoints.insert(id.to_string(), address),
                None => next.endpoints.remove(id),
            };
            self.persist(&mut state, next)?;
            let on = state.settings.enabled.iter().any(|e| e == id);
            Self::reset_slot(&mut state, id, on, fixed_name(id));
            self.info(id, &state.settings).ok_or("Unknown provider.")?
        };
        if id == "omlx" {
            self.epoch.fetch_add(1, Ordering::SeqCst);
            *self.omlx.lock().unwrap() = OmlxSession::default();
        }
        self.changed();
        Ok(info)
    }

    pub fn set_secret(&self, id: &str, secret: &str) -> Result<ProviderInfo, String> {
        if !is_parity_id(id) {
            return Err("Unknown provider.".into());
        }
        let secret = net::clean_secret(secret).ok_or("That key is empty or contains invalid characters.")?;
        {
            let state = self.state.lock().unwrap();
            self.info(id, &state.settings).ok_or("Unknown provider.")?;
            store_key(&self.key_id(id, &state.settings), &secret)?;
        }
        self.after_secret_change(id)
    }

    pub fn clear_secret(&self, id: &str) -> Result<ProviderInfo, String> {
        if !is_parity_id(id) {
            return Err("Unknown provider.".into());
        }
        {
            let state = self.state.lock().unwrap();
            self.info(id, &state.settings).ok_or("Unknown provider.")?;
            forget_key(&self.key_id(id, &state.settings))?;
        }
        self.after_secret_change(id)
    }

    fn after_secret_change(&self, id: &str) -> Result<ProviderInfo, String> {
        self.epoch.fetch_add(1, Ordering::SeqCst);
        if id == "ollama" {
            *self.ollama.lock().unwrap() = OllamaCloud::default();
        }
        if id == "omlx" {
            *self.omlx.lock().unwrap() = OmlxSession::default();
        }
        let info = {
            let mut state = self.state.lock().unwrap();
            let enabled = state.settings.enabled.iter().any(|e| e == id)
                || custom_id(id).is_some_and(|r| state.settings.custom.iter().any(|e| e.id == r && e.is_enabled));
            let name = match custom_id(id) {
                Some(r) => state.settings.custom.iter().find(|e| e.id == r).map(|e| e.name.clone()).unwrap_or_default(),
                None => fixed_name(id).to_string(),
            };
            Self::reset_slot(&mut state, id, enabled, &name);
            self.info(id, &state.settings).ok_or("Unknown provider.")?
        };
        self.changed();
        Ok(info)
    }

    pub fn request_refresh(&self, id: &str) -> bool {
        let now = now_ms();
        let requested = {
            let mut state = self.state.lock().unwrap();
            let enabled = state.settings.enabled.iter().any(|e| e == id)
                || custom_id(id).is_some_and(|r| state.settings.custom.iter().any(|e| e.id == r && e.is_enabled));
            if !enabled || state.backoff.get(id).is_some_and(|until| *until > now) {
                false
            } else {
                match state.slots.get_mut(id) {
                    Some(slot) if !slot.in_flight && now.saturating_sub(slot.last_attempt) >= refresh_floor_ms(id) => {
                        slot.next = 0;
                        true
                    }
                    _ => false,
                }
            }
        };
        requested
    }

    // MARK: Custom endpoints

    pub fn list_custom(&self) -> Vec<CustomEndpoint> {
        let mut list = self.state.lock().unwrap().settings.custom.clone();
        for endpoint in &mut list {
            endpoint.has_key = stored_key(&custom_key_id(endpoint)).is_some();
        }
        list
    }

    pub fn save_custom(&self, draft: CustomEndpoint, secret: Option<&str>) -> Result<CustomEndpoint, String> {
        let mut endpoint = custom::validate(draft)?;
        let new_secret = match secret.map(str::trim) {
            Some("") | None => None,
            Some(text) => Some(net::clean_secret(text).ok_or("That key contains invalid characters.")?),
        };
        // Refuse before anything changes, rather than saving an endpoint whose key silently went nowhere.
        if new_secret.is_some() && !SECRETS_AVAILABLE {
            return Err(NO_KEY_STORAGE.to_string());
        }
        let retired_key = {
            let mut state = self.state.lock().unwrap();
            let existing = state.settings.custom.iter().find(|e| e.id == endpoint.id).cloned();
            if endpoint.id.is_empty() {
                if state.settings.custom.len() >= custom::MAX_ENDPOINTS {
                    return Err("That is the most endpoints this list can hold.".into());
                }
                endpoint.id = custom::new_id(now_ms());
            } else if !custom::valid_id(&endpoint.id) || existing.is_none() {
                return Err("Unknown endpoint.".into());
            }
            let origin_changed = existing.as_ref().is_some_and(|old| net::origin(&old.base_url) != net::origin(&endpoint.base_url));
            if let Some(old) = &existing {
                // Figures this code keeps are not the form's to overwrite.
                endpoint.usage_history = old.usage_history.clone();
                endpoint.last_latency_ms = old.last_latency_ms;
                endpoint.last_health_status = old.last_health_status.clone();
                endpoint.last_checked_at = old.last_checked_at;
            }
            let mut next = state.settings.clone();
            match next.custom.iter_mut().find(|e| e.id == endpoint.id) {
                Some(slot) => *slot = endpoint.clone(),
                None => next.custom.push(endpoint.clone()),
            }
            let key_id = custom_key_id(&endpoint);
            let previous_key = stored_key(&key_id);
            let changed_key = new_secret.is_some() || secret.is_some_and(|s| s.trim().is_empty());
            if let Some(value) = new_secret.as_deref() { store_key(&key_id, value)?; }
            else if changed_key { forget_key(&key_id)?; }
            if let Err(error) = self.persist(&mut state, next) {
                if changed_key {
                    match previous_key { Some(ref key) => { store_key(&key_id,key)?; }, None => { forget_key(&key_id)?; } }
                }
                return Err(error);
            }
            let id = format!("custom:{}", endpoint.id);
            Self::reset_slot(&mut state, &id, endpoint.is_enabled, &endpoint.name);
            if origin_changed { existing.as_ref().map(custom_key_id) } else { None }
        };
        if let Some(id) = retired_key { forget_key(&id)?; }
        endpoint.has_key = stored_key(&custom_key_id(&endpoint)).is_some();
        self.changed();
        Ok(endpoint)
    }

    pub fn delete_custom(&self, id: &str) -> Result<(), String> {
        let full = format!("custom:{id}");
        let retired_key;
        {
            let mut state = self.state.lock().unwrap();
            let mut next = state.settings.clone();
            retired_key = next.custom.iter().find(|e| e.id == id).map(custom_key_id).ok_or("Unknown endpoint.")?;
            let before = next.custom.len();
            next.custom.retain(|e| e.id != id);
            if next.custom.len() == before {
                return Err("Unknown endpoint.".into());
            }
            self.persist(&mut state, next)?;
            state.slots.remove(&full);
            state.backoff.remove(&full);
            settings::save_backoff(&self.env.settings_dir, &state.backoff);
        }
        forget_key(&retired_key)?;
        self.changed();
        Ok(())
    }

    /// The stored key may only be used for the address it was saved with.
    fn draft_key(&self, draft: &CustomEndpoint, secret: Option<&str>) -> Option<String> {
        if let Some(text) = secret.and_then(net::clean_secret) {
            return Some(text);
        }
        let state = self.state.lock().unwrap();
        let saved = state.settings.custom.iter().find(|e| e.id == draft.id)?;
        (net::origin(&saved.base_url) == net::origin(&draft.base_url))
            .then(|| stored_key(&custom_key_id(saved)))
            .flatten()
    }

    pub fn test_custom(&self, draft: &CustomEndpoint, secret: Option<&str>) -> TestResult {
        let key = self.draft_key(draft, secret);
        let result = custom::test_endpoint(draft, key.as_deref());
        let mut state = self.state.lock().unwrap();
        let mut next = state.settings.clone();
        if let Some(saved) = next.custom.iter_mut().find(|e| e.id == draft.id) {
            saved.last_health_status = result.health.clone();
            saved.last_latency_ms = Some(result.latency_ms);
            saved.last_checked_at = Some(now_ms());
            if result.error.is_none() {
                saved.available_models = result.models.clone();
            }
            let _ = self.persist(&mut state, next);
        }
        result
    }

    pub fn detect_custom(&self, draft: &CustomEndpoint, secret: Option<&str>) -> Detection {
        custom::detect(draft, self.draft_key(draft, secret).as_deref())
    }

    pub fn presets(&self) -> Vec<Preset> {
        custom::presets()
    }

    pub fn scan(&self) -> Vec<Preset> {
        custom::scan_local()
    }

    pub fn import_mapping(&self, base: &str, text: &str) -> Result<Mapping, String> {
        custom::import_mapping(base, text)
    }

    // MARK: OpenCode bridge

    fn bridge_paths(&self) -> Result<(PathBuf, PathBuf), String> {
        let home = self.env.home.clone().ok_or("Your home folder could not be found.")?;
        let xdg = (self.env.vars)("XDG_CONFIG_HOME");
        Ok((bridge::plugins_dir(xdg.as_deref(), &home), home))
    }

    fn opencode_found(&self, home: &Path) -> bool {
        let names: &[&str] = if cfg!(windows) { &["opencode.exe", "opencode.cmd"] } else { &["opencode"] };
        let on_path = self.env.path_var.as_ref().is_some_and(|path| {
            std::env::split_paths(path)
                .filter(|d| d.is_absolute())
                .any(|dir| names.iter().any(|n| dir.join(n).is_file()))
        });
        on_path || home.join(".config").join("opencode").is_dir() || home.join(".local").join("share").join("opencode").is_dir()
    }

    pub fn bridge_status(&self) -> Result<bridge::Status, String> {
        let (dir, home) = self.bridge_paths()?;
        let found = self.opencode_found(&home);
        Ok(bridge::status(&dir, &home, &self.env.settings_dir.join("OpenCode"), found))
    }

    pub fn bridge_install(&self) -> Result<bridge::Status, String> {
        let (dir, _) = self.bridge_paths()?;
        bridge::install(&dir, bridge::TEMPLATE, now_ms())?;
        self.bridge_status()
    }

    pub fn bridge_remove(&self) -> Result<bridge::Status, String> {
        let (dir, _) = self.bridge_paths()?;
        bridge::remove(&dir)?;
        self.bridge_status()
    }

    // MARK: Polling

    fn key_id(&self, id: &str, settings: &Settings) -> String {
        if let Some(rest) = custom_id(id) {
            if let Some(endpoint) = settings.custom.iter().find(|e| e.id == rest) { return custom_key_id(endpoint); }
        }
        if matches!(id, "lmstudio" | "omlx" | "llamacpp") {
            return format!("{}@{:?}", id, self.resolved_endpoint(id, settings).and_then(|u| net::origin(&u)));
        }
        id.to_string()
    }

    fn key_for(&self, id: &str, settings: &Settings) -> Option<String> {
        env_names(id).iter().find_map(|n| self.env.var(n)).or_else(|| stored_key(&self.key_id(id, settings)))
    }

    fn read(&self, id: &str, settings: &Settings, now: u64) -> Result<Reading, Failure> {
        let env = &self.env;
        let endpoint = || self.resolved_endpoint(id, settings).unwrap_or_default();
        match id {
            "grokbot" => {
                let dirs = grokbot::candidate_dirs(env.appdata.as_deref(), env.local_appdata.as_deref());
                grokbot::load_first(&dirs, now)
            }
            "higgsfield" => {
                let binary = higgsfield::locate(&higgsfield::CliSearch {
                    override_path: (env.vars)("HIGGSFIELD_CLI_PATH"),
                    home: env.home.as_deref(),
                    appdata: env.appdata.as_deref(),
                    local_appdata: env.local_appdata.as_deref(),
                    path_var: env.path_var.clone(),
                });
                let mut files = Vec::new();
                if let Some(home) = env.home.as_deref() {
                    files.push(home.join(".config").join("higgsfield").join("credentials.json"));
                }
                if let Some(appdata) = env.appdata.as_deref() {
                    files.push(appdata.join("higgsfield").join("credentials.json"));
                }
                let token = self.key_for(id, settings).or_else(|| higgsfield::file_token(&files));
                higgsfield::read(binary, token, &env.urls.higgsfield, None)
            }
            "deepseek" => deepseek::read(&env.urls.deepseek, self.key_for(id, settings), now),
            "ollama" => {
                let key = self.key_for(id, settings);
                // Taken out rather than held: a settings change must not wait on this network call.
                let epoch = self.epoch.load(Ordering::SeqCst);
                let mut cloud = std::mem::take(&mut *self.ollama.lock().unwrap());
                let result = local::read_ollama(&endpoint(), &env.urls.ollama_cloud, key.as_deref(), &mut cloud, now);
                // A key changed meanwhile: this answer belongs to the old one, so it is not put back.
                if self.epoch.load(Ordering::SeqCst) == epoch {
                    *self.ollama.lock().unwrap() = cloud;
                }
                result
            }
            "lmstudio" => local::read_lmstudio(&endpoint(), self.key_for(id, settings).as_deref()),
            "omlx" => {
                let address = endpoint();
                let loopback = url::Url::parse(&address).is_ok_and(|u| net::is_loopback(&u));
                // A key found in a settings file on this PC only ever goes to this PC.
                let key = self.key_for(id, settings).or_else(|| {
                    loopback.then(|| env.home.as_deref().and_then(local::omlx_file_key)).flatten()
                });
                let epoch = self.epoch.load(Ordering::SeqCst);
                let mut session = std::mem::take(&mut *self.omlx.lock().unwrap());
                let result = local::read_omlx(&address, key.as_deref(), &mut session, now);
                if self.epoch.load(Ordering::SeqCst) == epoch {
                    *self.omlx.lock().unwrap() = session;
                }
                result
            }
            "llamacpp" => local::read_llamacpp(&endpoint(), self.key_for(id, settings).as_deref()),
            _ => {
                let rest = custom_id(id).ok_or_else(|| Failure::error("Unknown provider."))?;
                let e = settings
                    .custom
                    .iter()
                    .find(|e| e.id == rest)
                    .ok_or_else(|| Failure::error("Unknown endpoint."))?;
                custom::read(e, stored_key(&custom_key_id(e)).as_deref(), now)
            }
        }
    }

    /// Slots due now, marked in flight. At most `MAX_CONCURRENT` run at once.
    pub fn due(&self, now: u64) -> Vec<(String, u64)> {
        let mut state = self.state.lock().unwrap();
        let enabled: Vec<String> = state
            .settings
            .enabled
            .iter()
            .cloned()
            .chain(state.settings.custom.iter().filter(|e| e.is_enabled).map(|e| format!("custom:{}", e.id)))
            .collect();
        let mut jobs = Vec::new();
        for id in enabled {
            if state.running + jobs.len() >= MAX_CONCURRENT {
                break;
            }
            let waiting = state.backoff.get(&id).is_some_and(|until| *until > now);
            let Some(slot) = state.slots.get_mut(&id) else { continue };
            if !waiting && !slot.in_flight && now >= slot.next {
                slot.in_flight = true;
                slot.last_attempt = now;
                jobs.push((id, slot.generation));
            }
        }
        state.running += jobs.len();
        jobs
    }

    pub fn run_job(&self, id: &str, generation: u64) {
        let now = now_ms();
        let settings = self.state.lock().unwrap().settings.clone();
        let result = self.read(id, &settings, now);
        let changed = self.apply(id, generation, result, &settings, now);
        if changed {
            self.changed();
        }
    }

    fn apply(&self, id: &str, generation: u64, result: Result<Reading, Failure>, read_with: &Settings, now: u64) -> bool {
        let mut state = self.state.lock().unwrap();
        state.running = state.running.saturating_sub(1);
        let poll = poll_ms(id, read_with);
        let ollama_backoff = if id == "ollama" { self.ollama.lock().unwrap().backoff_until } else { 0 };
        let name = match custom_id(id) {
            Some(rest) => state.settings.custom.iter().find(|e| e.id == rest).map(|e| e.name.clone()).unwrap_or_default(),
            None => fixed_name(id).to_string(),
        };
        let Some(slot) = state.slots.get_mut(id) else { return false };
        slot.in_flight = false;
        // A provider turned off, re-pointed or re-keyed while this ran: its answer is about the past.
        if slot.generation != generation {
            return false;
        }
        let previous = slot.snapshot.clone();
        let deadline;
        let mut custom_tokens = None;
        match result {
            Ok(reading) => {
                custom_tokens = reading.custom_tokens;
                slot.rate_limits = 0;
                slot.next = now + poll;
                slot.snapshot = ParitySnapshot {
                    base: UsageSnapshot {
                        status: reading.status.unwrap_or("ok").into(),
                        windows: reading.windows,
                        fetched_at: reading.as_of.unwrap_or(now),
                        note: reading.note,
                        backoff_until: ollama_backoff,
                        ..Default::default()
                    },
                    display_name: name,
                    kind: kind_of(id).to_string(),
                    plan: reading.plan,
                    local_model: reading.local_model,
                    performance: reading.performance,
                    details: reading.details,
                };
                deadline = None;
            }
            Err(failure) => {
                let had_reading = !previous.base.windows.is_empty() && matches!(previous.base.status.as_str(), "ok" | "stale" | "backoff");
                let mut snapshot = if failure.transient && had_reading {
                    previous.clone()
                } else {
                    ParitySnapshot {
                        display_name: name,
                        kind: kind_of(id).to_string(),
                        ..Default::default()
                    }
                };
                snapshot.base.note = failure.note.clone();
                if failure.rate_limited {
                    let wait = net::backoff_secs(slot.rate_limits, failure.retry_after);
                    slot.rate_limits = slot.rate_limits.saturating_add(1);
                    let until = now.saturating_add(wait.saturating_mul(1000));
                    slot.next = until;
                    snapshot.base.status = "backoff".into();
                    snapshot.base.backoff_until = until;
                    deadline = Some(until);
                } else {
                    slot.next = now.saturating_add(failure.retry.saturating_mul(1000));
                    snapshot.base.status = if failure.transient && had_reading { "stale" } else { failure.status }.into();
                    snapshot.base.backoff_until = 0;
                    deadline = None;
                }
                slot.snapshot = snapshot;
            }
        }
        let same = serde_json::to_value(&previous).ok() == serde_json::to_value(&slot.snapshot).ok();
        match deadline {
            Some(until) => {
                state.backoff.insert(id.to_string(), until);
                settings::save_backoff(&self.env.settings_dir, &state.backoff);
            }
            None if state.backoff.remove(id).is_some() => {
                settings::save_backoff(&self.env.settings_dir, &state.backoff);
            }
            _ => {}
        }
        if let (Some(tokens), Some(rest)) = (custom_tokens, custom_id(id)) {
            let mut next = state.settings.clone();
            if let Some(endpoint) = next.custom.iter_mut().find(|e| e.id == rest) {
                if endpoint.current_tokens_used_m != Some(tokens) {
                    custom::record_history(endpoint, tokens, &utc_day(now));
                    let _ = self.persist(&mut state, next);
                }
            }
        }
        !same
    }

    pub fn tick_and_spawn(self: &Arc<Self>) {
        for (id, generation) in self.due(now_ms()) {
            let engine = self.clone();
            std::thread::spawn(move || engine.run_job(&id, generation));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers_plus::net::testkit;
    use serde_json::json;
    use std::collections::HashMap;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("matra-engine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env(dir: &Path, vars: &[(&str, &str)]) -> Env {
        let vars: HashMap<String, String> = vars.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        Env {
            settings_dir: dir.join("settings"),
            home: Some(dir.join("home")),
            appdata: Some(dir.join("appdata")),
            local_appdata: Some(dir.join("local")),
            vars: Arc::new(move |name| vars.get(name).cloned()),
            path_var: None,
            urls: Urls {
                ollama_cloud: "http://127.0.0.1:1/usage".into(),
                deepseek: "http://127.0.0.1:1/balance".into(),
                higgsfield: "http://127.0.0.1:1/ws".into(),
            },
        }
    }

    fn run(engine: &Engine, id: &str) {
        let jobs = engine.due(now_ms());
        let (_, generation) = jobs.into_iter().find(|(job, _)| job == id).expect("job due");
        engine.run_job(id, generation);
    }

    fn draft(base: &str) -> CustomEndpoint {
        serde_json::from_value(json!({"name":"E","base_url":base})).unwrap()
    }

    #[test]
    fn everything_is_off_and_absent_until_asked() {
        let dir = temp("off");
        let engine = Engine::new(env(&dir, &[]));
        let usage = engine.usage_map();
        assert_eq!(usage.len(), FIXED_IDS.len());
        assert!(usage.values().all(|s| s.base.status == "absent"));
        assert!(engine.due(now_ms()).is_empty());
        assert!(!dir.join("settings").exists());
        assert!(engine.registry().iter().all(|p| !p.enabled));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn snapshot_json_stays_compatible_with_usage_snapshot() {
        let dir = temp("shape");
        let engine = Engine::new(env(&dir, &[]));
        let value = serde_json::to_value(&engine.usage_map()["omlx"]).unwrap();
        for key in ["status", "windows", "fetched_at", "note", "backoff_until", "display_name", "kind"] {
            assert!(value.get(key).is_some(), "{key}");
        }
        assert!(value.get("local_model").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn enabling_persists_and_a_new_engine_remembers() {
        let dir = temp("persist");
        let engine = Engine::new(env(&dir, &[]));
        let info = engine.set_enabled("omlx", true).unwrap();
        assert!(info.enabled);
        assert_eq!(engine.enabled_ids(), ["omlx"]);
        let again = Engine::new(env(&dir, &[]));
        assert_eq!(again.enabled_ids(), ["omlx"]);
        assert_eq!(again.snapshot("omlx").status, "needsAuth");
        engine.set_enabled("omlx", false).unwrap();
        assert_eq!(engine.snapshot("omlx").status, "absent");
        assert!(engine.set_enabled("nonsense", true).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn endpoint_scopes_differ_by_where_the_runtime_lives() {
        let dir = temp("scope");
        let engine = Engine::new(env(&dir, &[]));
        assert!(engine.set_endpoint("ollama", Some("http://192.168.1.2:11434")).is_err());
        assert_eq!(engine.set_endpoint("ollama", Some("http://localhost:1111")).unwrap().endpoint.as_deref(), Some("http://127.0.0.1:1111"));
        assert_eq!(engine.set_endpoint("ollama", None).unwrap().endpoint.as_deref(), Some(local::OLLAMA_DEFAULT));
        let omlx = engine.set_endpoint("omlx", Some("http://192.168.1.20:8000")).unwrap();
        assert_eq!(omlx.endpoint.as_deref(), Some("http://192.168.1.20:8000"));
        assert_eq!(omlx.endpoint_scope, "network");
        assert!(engine.set_endpoint("deepseek", Some("http://127.0.0.1:1")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_storage_says_plainly_that_it_is_unavailable() {
        let dir = temp("nosave");
        let engine = Engine::new(env(&dir, &[]));
        assert!(!SECRETS_AVAILABLE);
        assert!(engine.registry().iter().all(|p| !p.accepts_secret && !p.has_stored_secret));
        let refused = engine.set_secret("deepseek", "synthetic").unwrap_err();
        assert!(refused.contains("unavailable"));
        assert!(!refused.contains("synthetic"));
        let denied = engine.save_custom(draft("https://x.example"), Some("synthetic")).unwrap_err();
        assert!(denied.contains("unavailable"));
        // Nothing was saved, and the secret reached no file.
        assert!(engine.list_custom().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_runtime_end_to_end() {
        let dir = temp("local");
        let server = testkit::serve(vec![("/api/v1/models", testkit::ok(
            r#"{"models":[{"type":"llm","key":"k","size_bytes":5,"loaded_instances":[{"id":"inst"}]}]}"#,
        ))]);
        let engine = Engine::new(env(&dir, &[]));
        engine.set_endpoint("lmstudio", Some(&server.base)).unwrap();
        engine.set_enabled("lmstudio", true).unwrap();
        run(&engine, "lmstudio");
        let usage = engine.usage_map();
        let snapshot = &usage["lmstudio"];
        assert_eq!(snapshot.base.status, "ok");
        assert_eq!(snapshot.kind, LOCAL);
        assert_eq!(snapshot.local_model.as_ref().unwrap().models[0].name, "inst");
        assert_eq!(snapshot.base.windows[0].count, Some(1));
        // Not due again inside its interval.
        assert!(engine.due(now_ms()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn env_key_reads_and_the_rate_limit_outlives_the_process() {
        let dir = temp("backoff");
        let server = testkit::serve(vec![("/balance", testkit::response("429 Too Many Requests", "Retry-After: 1200\r\n", ""))]);
        let mut environment = env(&dir, &[("DEEPSEEK_API_KEY", "synthetic")]);
        environment.urls.deepseek = format!("{}/balance", server.base);
        let engine = Engine::new(environment.clone());
        engine.set_enabled("deepseek", true).unwrap();
        run(&engine, "deepseek");
        let snapshot = engine.snapshot("deepseek");
        assert_eq!(snapshot.status, "backoff");
        assert!(snapshot.backoff_until > now_ms() + 1_000_000);
        assert!(!engine.request_refresh("deepseek"));
        assert!(engine.due(now_ms()).is_empty());
        let reborn = Engine::new(environment);
        assert_eq!(reborn.snapshot("deepseek").status, "backoff");
        assert!(reborn.due(now_ms()).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_transient_failure_keeps_the_reading_marked_stale_and_auth_loss_clears_it() {
        let dir = temp("stale");
        let mut environment = env(&dir, &[("DEEPSEEK_API_KEY", "synthetic")]);
        let server = testkit::serve(vec![
            ("/good", testkit::ok(r#"{"balance_infos":[{"currency":"USD","total_balance":"5"}]}"#)),
        ]);
        environment.urls.deepseek = format!("{}/good", server.base);
        let engine = Engine::new(environment);
        engine.set_enabled("deepseek", true).unwrap();
        run(&engine, "deepseek");
        assert_eq!(engine.snapshot("deepseek").status, "ok");
        let settings = engine.state.lock().unwrap().settings.clone();
        let generation = engine.state.lock().unwrap().slots["deepseek"].generation;
        engine.state.lock().unwrap().running += 1;
        engine.apply("deepseek", generation, Err(Failure::transient("Offline.")), &settings, now_ms());
        let stale = engine.snapshot("deepseek");
        assert_eq!(stale.status, "stale");
        assert_eq!(stale.windows.len(), 1);
        assert_eq!(stale.note, "Offline.");
        engine.state.lock().unwrap().running += 1;
        engine.apply("deepseek", generation, Err(Failure::needs_auth("Sign in.")), &settings, now_ms());
        let signed_out = engine.snapshot("deepseek");
        assert_eq!(signed_out.status, "needsAuth");
        assert!(signed_out.windows.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_answer_from_before_a_change_is_dropped() {
        let dir = temp("generation");
        let engine = Engine::new(env(&dir, &[("DEEPSEEK_API_KEY", "k")]));
        engine.set_enabled("deepseek", true).unwrap();
        let (_, generation) = engine.due(now_ms()).into_iter().next().unwrap();
        engine.set_enabled("deepseek", false).unwrap();
        let settings = engine.state.lock().unwrap().settings.clone();
        engine.apply("deepseek", generation, Ok(Reading::default()), &settings, now_ms());
        assert_eq!(engine.snapshot("deepseek").status, "absent");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn manual_refresh_has_a_floor() {
        let dir = temp("floor");
        let server = testkit::serve(vec![("/metrics", testkit::ok("llamacpp:prompt_tokens_total 1\nllamacpp:tokens_predicted_total 1\n"))]);
        let engine = Engine::new(env(&dir, &[]));
        engine.set_endpoint("llamacpp", Some(&server.base)).unwrap();
        assert!(!engine.request_refresh("llamacpp"));
        engine.set_enabled("llamacpp", true).unwrap();
        run(&engine, "llamacpp");
        assert!(!engine.request_refresh("llamacpp"));
        engine.state.lock().unwrap().slots.get_mut("llamacpp").unwrap().last_attempt = now_ms() - 6_000;
        assert!(engine.request_refresh("llamacpp"));
        assert!(!engine.request_refresh("unknown"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn custom_endpoint_life_cycle() {
        let dir = temp("custom");
        let server = testkit::serve(vec![
            ("/v1/models", testkit::ok(r#"{"data":[{"id":"model-a"}]}"#)),
            ("/rows", testkit::ok(r#"[{"m":"x","t":1000000}]"#)),
        ]);
        let engine = Engine::new(env(&dir, &[]));
        let mut e = draft(&format!("{}/v1", server.base));
        e.usage_source = "jsonEndpoint".into();
        e.tracking_unit = "tokens".into();
        e.usage_url = Some(format!("{}/rows", server.base));
        e.usage_records_path = Some(String::new());
        e.usage_model_field = Some("m".into());
        e.usage_token_field = Some("t".into());
        let saved = engine.save_custom(e, None).unwrap();
        assert!(!saved.id.is_empty());
        let id = format!("custom:{}", saved.id);
        assert!(is_parity_id(&id));
        assert!(engine.usage_map().contains_key(&id));

        let tested = engine.test_custom(&saved, None);
        assert_eq!(tested.models, ["model-a"]);
        assert_eq!(engine.list_custom()[0].available_models, ["model-a"]);

        run(&engine, &id);
        let usage = engine.usage_map();
        assert_eq!(usage[&id].base.status, "ok");
        assert_eq!(usage[&id].base.windows[0].count, Some(1_000_000));
        let listed = engine.list_custom();
        assert_eq!(listed[0].usage_history.len(), 1);
        assert_eq!(listed[0].current_tokens_used_m, Some(1.0));

        // Editing keeps what the reader recorded, and an unknown id is not silently created.
        let mut edit = listed[0].clone();
        edit.usage_history.clear();
        edit.name = "Renamed".into();
        assert_eq!(engine.save_custom(edit, None).unwrap().usage_history.len(), 1);
        let mut unknown = listed[0].clone();
        unknown.id = "missing".into();
        assert!(engine.save_custom(unknown, None).is_err());

        engine.set_enabled(&id, false).unwrap();
        assert_eq!(engine.snapshot(&id).status, "absent");
        engine.delete_custom(&saved.id).unwrap();
        assert!(!engine.usage_map().contains_key(&id));
        assert!(engine.delete_custom(&saved.id).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_secret_reaches_the_settings_file_or_the_ui() {
        let dir = temp("leak");
        let server = testkit::serve(vec![("/models", testkit::ok(r#"{"data":[]}"#))]);
        let engine = Engine::new(env(&dir, &[("DEEPSEEK_API_KEY", "synthetic-env-secret")]));
        let e = draft(&server.base);
        let saved = engine.save_custom(e, None).unwrap();
        engine.test_custom(&saved, Some("synthetic-draft-secret"));
        assert!(server.saw("synthetic-draft-secret"));
        engine.set_enabled("deepseek", true).unwrap();
        let text = std::fs::read_to_string(dir.join("settings/provider-settings.json")).unwrap();
        assert!(!text.contains("synthetic"));
        let ui = serde_json::to_string(&(engine.registry(), engine.usage_map(), engine.list_custom())).unwrap();
        assert!(!ui.contains("synthetic"));
        assert!(engine.registry().iter().find(|p| p.id == "deepseek").unwrap().has_env_secret);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bridge_only_moves_on_explicit_calls() {
        let dir = temp("bridge");
        let engine = Engine::new(env(&dir, &[]));
        let plugin = dir.join("home/.config/opencode/plugins/matra.js");
        let status = engine.bridge_status().unwrap();
        assert!(!status.installed);
        assert!(!plugin.exists());
        assert_eq!(status.location, "~/.config/opencode/plugins/matra.js");
        assert!(engine.bridge_install().unwrap().owned);
        assert!(plugin.exists());
        assert!(engine.bridge_status().unwrap().opencode_found);
        assert!(!engine.bridge_remove().unwrap().installed);
        assert!(!plugin.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn grokbot_reads_through_injected_paths() {
        let dir = temp("grok");
        let engine = Engine::new(env(&dir, &[]));
        engine.set_enabled("grokbot", true).unwrap();
        run(&engine, "grokbot");
        assert_eq!(engine.snapshot("grokbot").status, "needsAuth");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn utc_day_is_a_plain_date() {
        assert_eq!(utc_day(1_792_980_000_000), "2026-10-26");
    }
}
