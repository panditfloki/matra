//! Opt-in Windows notifications: usage thresholds, resets, finished work and agents waiting on you.
//!
//! Everything is off until the user turns it on. The rules port the Mac app's `ThresholdNotifier` and
//! `UsageResetWatcher`: a crossing rather than a level, the first live reading only records, and a
//! reading from an earlier run is never a baseline. Delivery sits behind `NotificationSink`, so the
//! rules can be tested without touching the notification system. A failure is counted and reported,
//! never shown as success, and "delivered" means only that Windows accepted the notification.
#![cfg_attr(not(windows), allow(dead_code))]
use crate::usage::{LimitWindow, UsageSnapshot};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// The installed shortcut's identity, equal to `identifier` in tauri.conf.json.
pub const APP_USER_MODEL_ID: &str = "com.dydxfx.matra.notch";

const POLL: Duration = Duration::from_secs(2);
/// A level holds until usage falls this far below it, so a reading that jitters around a threshold
/// does not announce the same crossing again.
const HYSTERESIS: f64 = 0.02;
/// Agents seen this soon after launch are recorded only: the transcript watcher replays recent work.
const AGENT_GRACE_MS: u64 = 20_000;
const AGENT_COOLDOWN_MS: u64 = 20_000;
const USAGE_COOLDOWN_MS: u64 = 60_000;
const MAX_ALERTS_PER_TICK: usize = 4;
/// A reading older than this is not current, whatever status it was saved with.
const MAX_READING_AGE_MS: u64 = 6 * 3600 * 1000;
const FUTURE_SKEW_MS: u64 = 60_000;
const MAX_MUTED: usize = 64;
const MAX_TEXT_CHARS: usize = 180;

// ---- Settings ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlertSettings {
    pub enabled: bool,
    pub usage_watch: bool,
    pub usage_critical: bool,
    pub usage_limit_session: bool,
    pub usage_limit_weekly: bool,
    pub usage_reset: bool,
    pub work_complete: bool,
    pub attention: bool,
    pub sound: bool,
    pub muted_providers: Vec<String>,
}

impl Default for AlertSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            usage_watch: false,
            usage_critical: false,
            usage_limit_session: false,
            usage_limit_weekly: false,
            usage_reset: false,
            work_complete: false,
            attention: false,
            sound: true,
            muted_providers: Vec::new(),
        }
    }
}

impl AlertSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.muted_providers.len() > MAX_MUTED
            || self.muted_providers.iter().any(|id| {
                id.is_empty()
                    || id.len() > 128
                    || !id.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_:. ".contains(&c))
            })
        {
            return Err("Invalid muted provider list.".into());
        }
        let mut unique = self.muted_providers.clone();
        unique.sort();
        unique.dedup();
        if unique.len() != self.muted_providers.len() {
            return Err("A provider was muted twice.".into());
        }
        Ok(())
    }

    fn allows(&self, kind: AlertKind) -> bool {
        self.enabled
            && match kind {
                AlertKind::UsageWatch => self.usage_watch,
                AlertKind::UsageCritical => self.usage_critical,
                AlertKind::LimitSession => self.usage_limit_session,
                AlertKind::LimitWeekly => self.usage_limit_weekly,
                AlertKind::UsageReset => self.usage_reset,
                AlertKind::WorkComplete => self.work_complete,
                AlertKind::Attention => self.attention,
                AlertKind::Test => true,
            }
    }

    fn is_muted(&self, provider: &str) -> bool {
        self.muted_providers.iter().any(|id| id == provider)
    }
}

fn settings_path() -> PathBuf {
    crate::config::config_path().with_file_name("alerts.json")
}

/// Anything missing or invalid means off: a damaged file must not start notifying.
pub fn load_from(path: &Path) -> AlertSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<AlertSettings>(&text).ok())
        .filter(|settings| settings.validate().is_ok())
        .unwrap_or_default()
}

pub fn save_to(path: &Path, settings: &AlertSettings) -> Result<(), String> {
    settings.validate()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    let pending = path.with_extension("json.pending");
    std::fs::write(&pending, text).map_err(|e| e.to_string())?;
    std::fs::rename(&pending, path).map_err(|e| {
        let _ = std::fs::remove_file(&pending);
        e.to_string()
    })
}

// ---- Decisions ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    UsageWatch,
    UsageCritical,
    LimitSession,
    LimitWeekly,
    UsageReset,
    WorkComplete,
    Attention,
    Test,
}

impl AlertKind {
    /// Higher goes first when a tick has more alerts than it may send.
    fn rank(self) -> u8 {
        match self {
            AlertKind::LimitSession | AlertKind::LimitWeekly => 6,
            AlertKind::Attention => 5,
            AlertKind::UsageCritical => 4,
            AlertKind::WorkComplete => 3,
            AlertKind::UsageWatch => 2,
            AlertKind::UsageReset => 1,
            AlertKind::Test => 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertEvent {
    pub kind: AlertKind,
    /// Provider id, or the agent's key
    pub subject: String,
    pub name: String,
    pub window_id: String,
    pub window_label: String,
    pub used_percent: i64,
    pub resets_at: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thresholds {
    pub watch: f64,
    pub critical: f64,
}

pub struct ProviderReading {
    pub id: String,
    pub name: String,
    pub snapshot: UsageSnapshot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Running,
    Attention,
    Done,
    Idle,
}

pub struct AgentReading {
    pub key: String,
    pub name: String,
    pub state: AgentState,
}

#[derive(Debug, Default)]
pub struct Observation {
    pub events: Vec<AlertEvent>,
    /// Alerts that were due but held back by a cooldown or the per-tick cap
    pub suppressed: u64,
}

#[derive(Debug, Clone)]
struct WindowTrack {
    fraction: f64,
    peak: f64,
    resets_at: Option<u64>,
    last_alerted_reset: Option<u64>,
    level: u8,
}

pub struct Engine {
    started_at: u64,
    thresholds: Option<Thresholds>,
    windows: HashMap<String, HashMap<String, WindowTrack>>,
    agents: HashMap<String, AgentState>,
    agents_baselined: bool,
    cooldowns: HashMap<(String, AlertKind), u64>,
}

/// 0 below Watch, 1 Watch, 2 Critical, 3 the limit itself.
fn raw_level(used: f64, t: Thresholds) -> u8 {
    if used >= 1.0 {
        3
    } else if used >= t.critical {
        2
    } else if used >= t.watch {
        1
    } else {
        0
    }
}

fn level_floor(level: u8, t: Thresholds) -> f64 {
    match level {
        3 => 1.0,
        2 => t.critical,
        1 => t.watch,
        _ => 0.0,
    }
}

fn held_level(used: f64, previous: u8, t: Thresholds) -> u8 {
    let raw = raw_level(used, t);
    if raw < previous && used >= level_floor(previous, t) - HYSTERESIS {
        previous
    } else {
        raw
    }
}

fn is_long_window(window: &LimitWindow) -> bool {
    let text = format!("{} {}", window.id, window.label).to_lowercase();
    ["week", "seven", "7d", "7-day", "month", "30d"].iter().any(|k| text.contains(k))
}

fn crossing_kind(level: u8, window: &LimitWindow) -> AlertKind {
    match level {
        3 if is_long_window(window) => AlertKind::LimitWeekly,
        3 => AlertKind::LimitSession,
        2 => AlertKind::UsageCritical,
        _ => AlertKind::UsageWatch,
    }
}

/// The highest crossed level the user wants to hear about, so a jump from 40% to 100% is one alert.
fn crossing_alert(previous: u8, level: u8, window: &LimitWindow, settings: &AlertSettings) -> Option<AlertKind> {
    (previous + 1..=level)
        .rev()
        .map(|candidate| crossing_kind(candidate, window))
        .find(|kind| settings.allows(*kind))
}

/// Ported from the Mac's `UsageResetWatcher`. A later reset time alone proves nothing, because a
/// relative countdown moves it on every refresh: the old window has to have actually elapsed.
fn is_reset(previous: &WindowTrack, fraction: f64, resets_at: Option<u64>, now: u64) -> bool {
    let elapsed = previous.resets_at.is_some_and(|at| at <= now);
    let rolled = elapsed
        && matches!((resets_at, previous.resets_at), (Some(next), Some(old)) if next > old)
        && previous.last_alerted_reset.map_or(true, |last| resets_at.is_some_and(|next| next > last));
    let dropped = fraction < previous.fraction
        && (previous.fraction - fraction >= 0.20 || (previous.peak >= 0.30 && fraction <= 0.10));
    let inferred = dropped && (previous.resets_at.is_none() || elapsed);
    (rolled || inferred) && previous.peak >= 0.15
}

/// An archived reading is not a baseline. At launch a snapshot comes back from disk with the status
/// it was saved with, so only a reading fetched since this process began counts as live.
fn is_live(snapshot: &UsageSnapshot, started_at: u64, now: u64) -> bool {
    snapshot.status == "ok"
        && snapshot.fetched_at >= started_at
        && snapshot.fetched_at <= now.saturating_add(FUTURE_SKEW_MS)
        && now.saturating_sub(snapshot.fetched_at) <= MAX_READING_AGE_MS
}

fn measurable(window: &LimitWindow) -> Option<f64> {
    (window.count.is_none() && window.used.is_finite() && window.used >= 0.0).then_some(window.used)
}

fn window_key(window: &LimitWindow) -> String {
    format!("{}|{}", window.group.as_deref().unwrap_or(""), window.id)
}

impl Engine {
    pub fn new(started_at: u64) -> Self {
        Self {
            started_at,
            thresholds: None,
            windows: HashMap::new(),
            agents: HashMap::new(),
            agents_baselined: false,
            cooldowns: HashMap::new(),
        }
    }

    fn forget(&mut self) {
        self.thresholds = None;
        self.windows.clear();
        self.agents.clear();
        self.agents_baselined = false;
    }

    /// State advances whatever the switches say, so turning one on later never announces the past.
    pub fn observe(
        &mut self,
        now: u64,
        settings: &AlertSettings,
        thresholds: Thresholds,
        providers: &[ProviderReading],
        agents: &[AgentReading],
    ) -> Observation {
        if !settings.enabled {
            self.forget();
            return Observation::default();
        }
        // New thresholds mean new levels. Measured against the old ones, every provider between
        // them would read as a crossing the moment a slider moved.
        if self.thresholds != Some(thresholds) {
            self.windows.clear();
            self.thresholds = Some(thresholds);
        }
        let mut due = self.observe_usage(now, settings, thresholds, providers);
        due.extend(self.observe_agents(now, settings, agents));

        let mut observation = Observation::default();
        due.sort_by_key(|event| std::cmp::Reverse(event.kind.rank()));
        for event in due {
            let cooldown = match event.kind {
                AlertKind::WorkComplete | AlertKind::Attention => AGENT_COOLDOWN_MS,
                _ => USAGE_COOLDOWN_MS,
            };
            let key = (event.subject.clone(), event.kind);
            let cooling = self.cooldowns.get(&key).is_some_and(|last| now.saturating_sub(*last) < cooldown);
            if cooling || observation.events.len() >= MAX_ALERTS_PER_TICK {
                observation.suppressed += 1;
                continue;
            }
            self.cooldowns.insert(key, now);
            observation.events.push(event);
        }
        self.cooldowns.retain(|_, last| now.saturating_sub(*last) < USAGE_COOLDOWN_MS.max(AGENT_COOLDOWN_MS));
        observation
    }

    fn observe_usage(
        &mut self,
        now: u64,
        settings: &AlertSettings,
        thresholds: Thresholds,
        providers: &[ProviderReading],
    ) -> Vec<AlertEvent> {
        self.windows.retain(|id, _| providers.iter().any(|provider| &provider.id == id));
        let mut events = Vec::new();
        for provider in providers {
            if !is_live(&provider.snapshot, self.started_at, now) {
                self.windows.remove(&provider.id);
                continue;
            }
            let previous = self.windows.remove(&provider.id).unwrap_or_default();
            let mut current = HashMap::new();
            let mut best: Option<AlertEvent> = None;
            for window in &provider.snapshot.windows {
                let Some(fraction) = measurable(window) else { continue };
                let key = window_key(window);
                let Some(mut track) = previous.get(&key).cloned() else {
                    current.insert(key, WindowTrack {
                        fraction,
                        peak: fraction,
                        resets_at: window.resets_at,
                        last_alerted_reset: window.resets_at,
                        level: raw_level(fraction, thresholds),
                    });
                    continue;
                };
                let kind = if is_reset(&track, fraction, window.resets_at, now) {
                    track.peak = fraction;
                    track.level = raw_level(fraction, thresholds);
                    track.last_alerted_reset = window.resets_at;
                    settings.allows(AlertKind::UsageReset).then_some(AlertKind::UsageReset)
                } else {
                    track.peak = track.peak.max(fraction);
                    let level = held_level(fraction, track.level, thresholds);
                    let crossed = (level > track.level)
                        .then(|| crossing_alert(track.level, level, window, settings))
                        .flatten();
                    track.level = level;
                    crossed
                };
                track.fraction = fraction;
                track.resets_at = window.resets_at;
                current.insert(key, track);
                let Some(kind) = kind else { continue };
                if settings.is_muted(&provider.id) {
                    continue;
                }
                // One notification per provider per reading, however many windows moved at once.
                if best.as_ref().map_or(true, |event| kind.rank() > event.kind.rank()) {
                    best = Some(AlertEvent {
                        kind,
                        subject: provider.id.clone(),
                        name: provider.name.clone(),
                        window_id: window_key(window),
                        window_label: window.label.clone(),
                        used_percent: (fraction * 100.0).round() as i64,
                        resets_at: window.resets_at,
                    });
                }
            }
            self.windows.insert(provider.id.clone(), current);
            events.extend(best);
        }
        events
    }

    fn observe_agents(&mut self, now: u64, settings: &AlertSettings, agents: &[AgentReading]) -> Vec<AlertEvent> {
        let quiet = !self.agents_baselined || now < self.started_at.saturating_add(AGENT_GRACE_MS);
        let mut next = HashMap::with_capacity(agents.len());
        let mut events = Vec::new();
        for agent in agents {
            let previous = self.agents.get(&agent.key).copied();
            next.insert(agent.key.clone(), agent.state);
            if quiet || previous == Some(agent.state) {
                continue;
            }
            let kind = match (agent.state, previous) {
                (AgentState::Attention, _) => AlertKind::Attention,
                // A session first seen already finished shows no work this run watched happen.
                (AgentState::Done, Some(_)) => AlertKind::WorkComplete,
                _ => continue,
            };
            if settings.allows(kind) {
                events.push(AlertEvent {
                    kind,
                    subject: agent.key.clone(),
                    name: agent.name.clone(),
                    window_id: String::new(),
                    window_label: String::new(),
                    used_percent: 0,
                    resets_at: None,
                });
            }
        }
        self.agents = next;
        self.agents_baselined = true;
        events
    }
}

// ---- Wording ----

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub title: String,
    pub body: String,
    pub tag: String,
    pub silent: bool,
}

fn resets_clause(event: &AlertEvent, clock: &dyn Fn(u64) -> Option<String>) -> String {
    let window = event.window_label.to_lowercase();
    match event.resets_at.and_then(clock) {
        Some(time) => format!("Its {window} limit is spent. Resets {time}."),
        None => format!("Its {window} limit is spent."),
    }
}

/// Agents are named by session title or provider only. A waiting agent's message can quote a command
/// or a prompt, and a notification can appear on a lock screen.
pub fn toast_for(event: &AlertEvent, settings: &AlertSettings, clock: &dyn Fn(u64) -> Option<String>) -> Toast {
    let name = &event.name;
    let window = event.window_label.to_lowercase();
    let (title, body) = match event.kind {
        AlertKind::UsageWatch | AlertKind::UsageCritical => (
            format!("{name} is at {}%", event.used_percent),
            format!("{}% of its {window} limit used.", event.used_percent),
        ),
        AlertKind::LimitSession | AlertKind::LimitWeekly => {
            (format!("{name} limit reached"), resets_clause(event, clock))
        }
        AlertKind::UsageReset => (format!("{name} has reset"), format!("Its {window} limit is available again.")),
        AlertKind::WorkComplete => ("Work finished".to_string(), name.clone()),
        AlertKind::Attention => ("Waiting on you".to_string(), name.clone()),
        AlertKind::Test => ("Mātrā test notification".to_string(), "Windows accepted this test.".to_string()),
    };
    let digest = Sha256::digest(format!("{:?}|{}|{}", event.kind, event.subject, event.window_id).as_bytes());
    let tag = digest.iter().take(16).map(|byte| format!("{byte:02x}")).collect();
    Toast { title, body, tag, silent: !settings.sound }
}

fn xml_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().filter(|c| !c.is_control()).take(MAX_TEXT_CHARS) {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn toast_xml(toast: &Toast) -> String {
    let audio = if toast.silent { "<audio silent=\"true\"/>" } else { "" };
    format!(
        "<toast><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding></visual>{audio}</toast>",
        xml_text(&toast.title),
        xml_text(&toast.body)
    )
}

// ---- Delivery ----

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Availability {
    pub os_state: String,
    pub can_deliver: bool,
    pub message: String,
}

impl Availability {
    fn new(os_state: &str, can_deliver: bool, message: &str) -> Self {
        Self { os_state: os_state.into(), can_deliver, message: message.into() }
    }
}

pub trait NotificationSink: Send + Sync {
    fn availability(&self) -> Availability;
    /// Ok only when the operating system accepted the notification.
    fn show(&self, toast: &Toast) -> Result<(), String>;
}

#[cfg(windows)]
mod windows_sink {
    use super::{toast_xml, Availability, NotificationSink, Toast, APP_USER_MODEL_ID};
    use std::time::Duration;
    use windows::core::{Result as WinResult, HSTRING, PCWSTR};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{
        NotificationSetting, ToastFailedEventArgs, ToastNotification, ToastNotificationManager, ToastNotifier,
    };
    use windows::Win32::System::Com::{CoInitializeEx, IBindCtx, COINIT_MULTITHREADED};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows::Win32::UI::Shell::{IShellItem, SHCreateItemFromParsingName};

    /// Failures arrive on an event shortly after Show returns. Past this, Windows has taken it.
    const FAILED_EVENT_WAIT: Duration = Duration::from_millis(750);

    pub struct ToastSink;

    /// Each call gets its own multithreaded COM thread, whatever apartment the caller is in.
    fn on_com_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> Option<T> {
        std::thread::spawn(move || {
            let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            work()
        })
        .join()
        .ok()
    }

    /// Windows accepts a notification for an unregistered identity and shows nothing, so the identity
    /// has to be confirmed first: the Start Menu shortcut's AUMID is what puts it in AppsFolder.
    fn identity_registered() -> bool {
        let path = HSTRING::from(format!("shell:AppsFolder\\{APP_USER_MODEL_ID}"));
        unsafe { SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(path.as_ptr()), None::<&IBindCtx>) }.is_ok()
    }

    fn notifier() -> WinResult<ToastNotifier> {
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_USER_MODEL_ID))
    }

    fn process_elevated() -> Option<bool> {
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).ok()?;
            let mut elevation = TOKEN_ELEVATION::default();
            let mut length = 0;
            let result = GetTokenInformation(token, TokenElevation,
                Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32, &mut length);
            let _ = CloseHandle(token);
            result.ok().map(|()| elevation.TokenIsElevated != 0)
        }
    }

    fn read_availability() -> Availability {
        // Windows app notifications cannot be delivered from an elevated process.
        // Report the actionable cause instead of the service's opaque 0x80070490.
        match process_elevated() {
            Some(true) => return Availability::new("elevated", false,
                "Windows cannot show app notifications while Mātrā runs as administrator. Quit it and reopen it normally from Start."),
            None => return Availability::new("error", false, "Windows could not check this app's notification privileges."),
            Some(false) => {}
        }
        if !identity_registered() {
            return Availability::new(
                "identity_missing",
                false,
                "This copy has no installed Start Menu identity, so Windows would not show its notifications. Use the installed app.",
            );
        }
        let setting = match notifier().and_then(|notifier| notifier.Setting()) {
            Ok(setting) => setting,
            Err(error) => {
                return Availability::new(
                    "error",
                    false,
                    &format!("Windows did not report its notification setting (0x{:08X}).", error.code().0),
                )
            }
        };
        match setting {
            NotificationSetting::Enabled => Availability::new("enabled", true, "Windows notifications are on for Mātrā."),
            NotificationSetting::DisabledForApplication => Availability::new(
                "disabled_app",
                false,
                "Notifications are off for Mātrā in Windows Settings > System > Notifications.",
            ),
            NotificationSetting::DisabledForUser => {
                Availability::new("disabled_user", false, "Notifications are off in Windows Settings > System > Notifications.")
            }
            NotificationSetting::DisabledByGroupPolicy => {
                Availability::new("disabled_policy", false, "An organisation policy has turned off notifications on this PC.")
            }
            NotificationSetting::DisabledByManifest => {
                Availability::new("disabled_manifest", false, "This build of Mātrā is not allowed to send notifications.")
            }
            _ => Availability::new("error", false, "Windows reported an unknown notification setting."),
        }
    }

    fn send(toast: &Toast) -> Result<(), String> {
        let available = read_availability();
        if !available.can_deliver {
            return Err(available.message);
        }
        let hresult = |error: windows::core::Error| format!("Windows refused the notification (0x{:08X}).", error.code().0);
        let document = XmlDocument::new().map_err(hresult)?;
        document.LoadXml(&HSTRING::from(toast_xml(toast))).map_err(hresult)?;
        let notification = ToastNotification::CreateToastNotification(&document).map_err(hresult)?;
        notification.SetTag(&HSTRING::from(toast.tag.as_str())).map_err(hresult)?;
        notification.SetGroup(&HSTRING::from("matra")).map_err(hresult)?;
        let (failed, failure) = std::sync::mpsc::channel::<i32>();
        notification
            .Failed(&TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(move |_, args| {
                let code = args.as_ref().and_then(|args| args.ErrorCode().ok()).map_or(-1, |code| code.0);
                let _ = failed.send(code);
                Ok(())
            }))
            .map_err(hresult)?;
        notifier().map_err(hresult)?.Show(&notification).map_err(hresult)?;
        match failure.recv_timeout(FAILED_EVENT_WAIT) {
            Ok(code) => Err(format!("Windows could not show the notification (0x{code:08X}).")),
            Err(_) => Ok(()),
        }
    }

    impl NotificationSink for ToastSink {
        fn availability(&self) -> Availability {
            on_com_thread(read_availability)
                .unwrap_or_else(|| Availability::new("error", false, "The notification check stopped unexpectedly."))
        }
        fn show(&self, toast: &Toast) -> Result<(), String> {
            let toast = toast.clone();
            on_com_thread(move || send(&toast)).unwrap_or_else(|| Err("The notification attempt stopped unexpectedly.".into()))
        }
    }
}

#[cfg(not(windows))]
struct UnsupportedSink;

#[cfg(not(windows))]
impl NotificationSink for UnsupportedSink {
    fn availability(&self) -> Availability {
        Availability::new("unsupported", false, "Notifications from this module are available on Windows only.")
    }
    fn show(&self, _toast: &Toast) -> Result<(), String> {
        Err(self.availability().message)
    }
}

fn platform_sink() -> Box<dyn NotificationSink> {
    #[cfg(windows)]
    return Box::new(windows_sink::ToastSink);
    #[cfg(not(windows))]
    return Box::new(UnsupportedSink);
}

// ---- Delivery records ----

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeliveryRecord {
    pub kind: AlertKind,
    pub provider: String,
    pub at: u64,
    pub ok: bool,
    pub error: String,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub delivered: u64,
    pub failed: u64,
    pub suppressed: u64,
    pub last: Option<DeliveryRecord>,
}

/// Only provider ids reach the status. A session's key and title stay out of it.
fn subject_for_status(event: &AlertEvent) -> String {
    match event.kind {
        AlertKind::WorkComplete | AlertKind::Attention => String::new(),
        _ => event.subject.clone(),
    }
}

/// The sink can take most of a second, so the record is locked only after it returns.
pub fn deliver(
    sink: &dyn NotificationSink,
    stats: &Mutex<Stats>,
    event: &AlertEvent,
    settings: &AlertSettings,
    clock: &dyn Fn(u64) -> Option<String>,
    now: u64,
) -> Result<(), String> {
    let result = sink.show(&toast_for(event, settings, clock));
    let mut stats = stats.lock().unwrap();
    match &result {
        Ok(()) => stats.delivered += 1,
        Err(_) => stats.failed += 1,
    }
    stats.last = Some(DeliveryRecord {
        kind: event.kind,
        provider: subject_for_status(event),
        at: now,
        ok: result.is_ok(),
        error: result.as_ref().err().cloned().unwrap_or_default(),
    });
    result
}

// ---- App surface ----

struct Runtime {
    settings: Mutex<AlertSettings>,
    stats: Mutex<Stats>,
    sink: Box<dyn NotificationSink>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderName {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlertStatus {
    #[serde(flatten)]
    pub availability: Availability,
    pub watch: f64,
    pub critical: f64,
    pub delivered: u64,
    pub failed: u64,
    pub suppressed: u64,
    pub last: Option<DeliveryRecord>,
    pub providers: Vec<ProviderName>,
}

fn clock_text(at_ms: u64) -> Option<String> {
    use chrono::TimeZone;
    let local = chrono::Local.timestamp_millis_opt(i64::try_from(at_ms).ok()?).single()?;
    let pattern = if crate::i18n::clock_24h() { "%H:%M" } else { "%-I:%M %p" };
    Some(local.format(pattern).to_string())
}

fn thresholds_of(app: &AppHandle) -> Thresholds {
    let colors = app.state::<crate::AppState>().cfg.lock().unwrap().quota_colors.clone().sanitized();
    Thresholds { watch: colors.watch, critical: colors.critical }
}

fn agent_state_of_session(state: &str) -> AgentState {
    match state {
        crate::state::ST_RUNNING => AgentState::Running,
        crate::state::ST_ATTENTION => AgentState::Attention,
        crate::state::ST_DONE => AgentState::Done,
        _ => AgentState::Idle,
    }
}

fn agent_state_of_activity(state: &str) -> AgentState {
    match state {
        "busy" => AgentState::Running,
        "waiting" => AgentState::Attention,
        "done" => AgentState::Done,
        _ => AgentState::Idle,
    }
}

fn read_agents(app: &AppHandle) -> Vec<AgentReading> {
    let st = app.state::<crate::AppState>();
    let sessions = st.store.lock().unwrap().snapshot("", "", false, false).sessions;
    let mut agents: Vec<AgentReading> = sessions
        .into_iter()
        .map(|session| AgentReading {
            key: format!("session:{}", session.id),
            name: session.title,
            state: agent_state_of_session(&session.state),
        })
        .collect();
    let activity = st.activity.lock().unwrap().clone();
    agents.extend(activity.into_iter().map(|item| AgentReading {
        key: format!("activity:{}:{}", item.provider, item.name),
        // An activity's name can come from a thread title, which can come from a prompt.
        name: crate::provider_label(&item.provider).to_string(),
        state: agent_state_of_activity(&item.state),
    }));
    agents
}

fn read_providers(app: &AppHandle) -> Vec<ProviderReading> {
    crate::provider_ids()
        .into_iter()
        .map(|id| ProviderReading {
            snapshot: crate::snapshot_of(app, &id),
            name: crate::provider_label(&id).to_string(),
            id,
        })
        .collect()
}

fn status_of(app: &AppHandle, runtime: &Runtime) -> AlertStatus {
    let thresholds = thresholds_of(app);
    let stats = runtime.stats.lock().unwrap().clone();
    AlertStatus {
        availability: runtime.sink.availability(),
        watch: thresholds.watch,
        critical: thresholds.critical,
        delivered: stats.delivered,
        failed: stats.failed,
        suppressed: stats.suppressed,
        last: stats.last,
        providers: crate::provider_ids()
            .into_iter()
            .map(|id| ProviderName { name: crate::provider_label(&id).to_string(), id })
            .collect(),
    }
}

fn runtime(app: &AppHandle) -> Result<tauri::State<'_, Runtime>, String> {
    app.try_state::<Runtime>().ok_or_else(|| "Notifications are not ready yet.".to_string())
}

fn publish_status(app: &AppHandle, runtime: &Runtime) -> AlertStatus {
    let status = status_of(app, runtime);
    let _ = app.emit("alert_status", &status);
    status
}

/// Registers the state and starts the one background thread. Call once from `setup`.
pub fn start(app: AppHandle) {
    let started_at = crate::now_ms();
    app.manage(Runtime {
        settings: Mutex::new(load_from(&settings_path())),
        stats: Mutex::new(Stats::default()),
        sink: platform_sink(),
    });
    let spawned = std::thread::Builder::new().name("matra-alerts".into()).spawn(move || {
        let mut engine = Engine::new(started_at);
        loop {
            std::thread::sleep(POLL);
            tick(&app, &mut engine);
        }
    });
    if let Err(error) = spawned {
        eprintln!("alerts: the monitor did not start: {error}");
    }
}

fn tick(app: &AppHandle, engine: &mut Engine) {
    let Ok(runtime) = runtime(app) else { return };
    let settings = runtime.settings.lock().unwrap().clone();
    let observation = if settings.enabled {
        engine.observe(crate::now_ms(), &settings, thresholds_of(app), &read_providers(app), &read_agents(app))
    } else {
        engine.observe(crate::now_ms(), &settings, Thresholds { watch: 0.5, critical: 0.7 }, &[], &[])
    };
    if observation.events.is_empty() && observation.suppressed == 0 {
        return;
    }
    runtime.stats.lock().unwrap().suppressed += observation.suppressed;
    for event in &observation.events {
        if let Err(error) = deliver(runtime.sink.as_ref(), &runtime.stats, event, &settings, &clock_text, crate::now_ms()) {
            eprintln!("alerts: {:?} not delivered: {error}", event.kind);
        }
    }
    publish_status(app, &runtime);
}

#[tauri::command]
pub fn get_alert_settings(app: AppHandle) -> Result<AlertSettings, String> {
    Ok(runtime(&app)?.settings.lock().unwrap().clone())
}

#[tauri::command]
pub fn set_alert_settings(app: AppHandle, settings: AlertSettings) -> Result<AlertSettings, String> {
    settings.validate()?;
    let runtime = runtime(&app)?;
    {
        let mut current = runtime.settings.lock().unwrap();
        save_to(&settings_path(), &settings).map_err(|_| "Could not save notification preferences.".to_string())?;
        *current = settings.clone();
    }
    let _ = app.emit("alert_settings", &settings);
    Ok(settings)
}

#[tauri::command(async)]
pub fn get_alert_status(app: AppHandle) -> Result<AlertStatus, String> {
    let runtime = runtime(&app)?;
    Ok(status_of(&app, &runtime))
}

/// The explicit button only. Works with the master switch off, as the Mac's test does.
#[tauri::command(async)]
pub fn send_test_alert(app: AppHandle) -> Result<AlertStatus, String> {
    let runtime = runtime(&app)?;
    let settings = runtime.settings.lock().unwrap().clone();
    let event = AlertEvent {
        kind: AlertKind::Test,
        subject: "test".into(),
        name: String::new(),
        window_id: String::new(),
        window_label: String::new(),
        used_percent: 0,
        resets_at: None,
    };
    let result = deliver(runtime.sink.as_ref(), &runtime.stats, &event, &settings, &clock_text, crate::now_ms());
    let status = publish_status(&app, &runtime);
    result.map(|()| status)
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: u64 = 1_000_000_000;
    const T: Thresholds = Thresholds { watch: 0.5, critical: 0.7 };

    fn all_on() -> AlertSettings {
        AlertSettings {
            enabled: true,
            usage_watch: true,
            usage_critical: true,
            usage_limit_session: true,
            usage_limit_weekly: true,
            usage_reset: true,
            work_complete: true,
            attention: true,
            sound: true,
            muted_providers: Vec::new(),
        }
    }

    fn window(id: &str, label: &str, used: f64, resets_at: Option<u64>) -> LimitWindow {
        LimitWindow { id: id.into(), label: label.into(), used, resets_at, count: None, derived: false, group: None }
    }

    fn reading(id: &str, fetched_at: u64, windows: Vec<LimitWindow>) -> ProviderReading {
        ProviderReading {
            id: id.into(),
            name: id.to_uppercase(),
            snapshot: UsageSnapshot { status: "ok".into(), windows, fetched_at, ..Default::default() },
        }
    }

    fn session_window(used: f64) -> Vec<LimitWindow> {
        vec![window("five_hour", "5-hour", used, None)]
    }

    /// A far-off time per step, so cooldowns do not hide what a rule decided.
    fn at(step: u64) -> u64 {
        START + 100_000 + step * 300_000
    }

    fn step(engine: &mut Engine, n: u64, settings: &AlertSettings, used: f64) -> Vec<AlertKind> {
        let now = at(n);
        engine
            .observe(now, settings, T, &[reading("claude", now, session_window(used))], &[])
            .events
            .iter()
            .map(|event| event.kind)
            .collect()
    }

    fn agent(key: &str, state: AgentState) -> AgentReading {
        AgentReading { key: key.into(), name: "project · ab12".into(), state }
    }

    fn agents_step(engine: &mut Engine, n: u64, settings: &AlertSettings, agents: &[AgentReading]) -> Vec<AlertKind> {
        engine.observe(at(n), settings, T, &[], agents).events.iter().map(|event| event.kind).collect()
    }

    #[test]
    fn everything_is_off_by_default_and_a_damaged_file_stays_off() {
        let defaults = AlertSettings::default();
        assert!(!defaults.enabled);
        for kind in [
            AlertKind::UsageWatch,
            AlertKind::UsageCritical,
            AlertKind::LimitSession,
            AlertKind::LimitWeekly,
            AlertKind::UsageReset,
            AlertKind::WorkComplete,
            AlertKind::Attention,
        ] {
            assert!(!defaults.allows(kind));
        }
        let dir = std::env::temp_dir().join(format!("matra-alerts-{}", std::process::id()));
        let path = dir.join("alerts.json");
        assert_eq!(load_from(&path), defaults);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(load_from(&path), defaults);
        let bad = serde_json::json!({ "enabled": true, "muted_providers": ["a", "a"] });
        std::fs::write(&path, bad.to_string()).unwrap();
        assert_eq!(load_from(&path), defaults);

        let chosen = AlertSettings { muted_providers: vec!["custom:one".into()], ..all_on() };
        save_to(&path, &chosen).unwrap();
        assert_eq!(load_from(&path), chosen);
        assert!(!path.with_extension("json.pending").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_save_keeps_the_previous_file() {
        let dir = std::env::temp_dir().join(format!("matra-alerts-invalid-{}", std::process::id()));
        let path = dir.join("alerts.json");
        save_to(&path, &all_on()).unwrap();
        let invalid = AlertSettings { muted_providers: vec![String::new()], ..AlertSettings::default() };
        assert!(save_to(&path, &invalid).is_err());
        assert_eq!(load_from(&path), all_on());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_first_reading_only_records_even_past_every_threshold() {
        let mut engine = Engine::new(START);
        assert!(step(&mut engine, 0, &all_on(), 1.0).is_empty());
        assert!(step(&mut engine, 1, &all_on(), 1.0).is_empty());
    }

    #[test]
    fn a_reading_from_before_this_run_is_not_a_baseline() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        let archived = reading("claude", START - 60_000, session_window(0.2));
        assert!(engine.observe(at(0), &settings, T, &[archived], &[]).events.is_empty());
        // Measured against the archive this would be a crossing and ring on every start.
        assert!(step(&mut engine, 1, &settings, 0.95).is_empty());
    }

    #[test]
    fn crossings_fire_once_each_in_order() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        step(&mut engine, 0, &settings, 0.2);
        assert_eq!(step(&mut engine, 1, &settings, 0.55), [AlertKind::UsageWatch]);
        assert!(step(&mut engine, 2, &settings, 0.6).is_empty());
        assert_eq!(step(&mut engine, 3, &settings, 0.75), [AlertKind::UsageCritical]);
        assert!(step(&mut engine, 4, &settings, 0.9).is_empty());
        assert_eq!(step(&mut engine, 5, &settings, 1.0), [AlertKind::LimitSession]);
        assert!(step(&mut engine, 6, &settings, 1.0).is_empty());
    }

    #[test]
    fn a_jump_is_one_alert_at_the_highest_wanted_level() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        step(&mut engine, 0, &settings, 0.1);
        assert_eq!(step(&mut engine, 1, &settings, 1.0), [AlertKind::LimitSession]);

        let mut engine = Engine::new(START);
        let no_limit = AlertSettings { usage_limit_session: false, ..all_on() };
        step(&mut engine, 0, &no_limit, 0.1);
        assert_eq!(step(&mut engine, 1, &no_limit, 1.0), [AlertKind::UsageCritical]);
    }

    #[test]
    fn jitter_around_a_threshold_does_not_alert_again() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        step(&mut engine, 0, &settings, 0.4);
        assert_eq!(step(&mut engine, 1, &settings, 0.5), [AlertKind::UsageWatch]);
        assert!(step(&mut engine, 2, &settings, 0.49).is_empty());
        assert!(step(&mut engine, 3, &settings, 0.5).is_empty());
        // Well clear of it and back is a new crossing.
        assert!(step(&mut engine, 4, &settings, 0.47).is_empty());
        assert_eq!(step(&mut engine, 5, &settings, 0.51), [AlertKind::UsageWatch]);
    }

    #[test]
    fn switches_and_mutes_decide_delivery_and_state_still_advances() {
        let mut engine = Engine::new(START);
        let quiet = AlertSettings { enabled: true, ..AlertSettings::default() };
        step(&mut engine, 0, &quiet, 0.2);
        assert!(step(&mut engine, 1, &quiet, 0.8).is_empty());
        // Turning the switch on later must not announce a crossing that already happened.
        assert!(step(&mut engine, 2, &all_on(), 0.8).is_empty());

        let mut engine = Engine::new(START);
        let muted = AlertSettings { muted_providers: vec!["claude".into()], ..all_on() };
        step(&mut engine, 0, &muted, 0.2);
        assert!(step(&mut engine, 1, &muted, 0.8).is_empty());
        assert!(step(&mut engine, 2, &all_on(), 0.8).is_empty());
    }

    #[test]
    fn the_master_switch_forgets_and_the_next_reading_rebaselines() {
        let mut engine = Engine::new(START);
        step(&mut engine, 0, &all_on(), 0.2);
        let off = AlertSettings { enabled: false, ..all_on() };
        assert!(step(&mut engine, 1, &off, 0.9).is_empty());
        assert!(step(&mut engine, 2, &all_on(), 0.95).is_empty());
        assert_eq!(step(&mut engine, 3, &all_on(), 1.0), [AlertKind::LimitSession]);
    }

    #[test]
    fn stale_and_failing_readings_forget_the_provider() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        step(&mut engine, 0, &settings, 0.2);
        for status in ["stale", "backoff", "needsAuth", "error", "absent"] {
            let mut failing = reading("claude", at(1), session_window(0.9));
            failing.snapshot.status = status.into();
            assert!(engine.observe(at(1), &settings, T, &[failing], &[]).events.is_empty());
            assert!(step(&mut engine, 2, &settings, 0.95).is_empty(), "{status}");
            step(&mut engine, 3, &settings, 0.2);
        }
        let old = reading("claude", at(5).saturating_sub(MAX_READING_AGE_MS + 1).max(START), session_window(0.99));
        assert!(engine.observe(at(5) + MAX_READING_AGE_MS, &settings, T, &[old], &[]).events.is_empty());
    }

    #[test]
    fn changing_thresholds_is_not_a_crossing() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        let now = at(0);
        engine.observe(now, &settings, T, &[reading("claude", now, session_window(0.45))], &[]);
        let lower = Thresholds { watch: 0.3, critical: 0.4 };
        let now = at(1);
        assert!(engine.observe(now, &settings, lower, &[reading("claude", now, session_window(0.45))], &[]).events.is_empty());
    }

    #[test]
    fn count_windows_and_nonsense_values_are_ignored() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        let mut count = window("requests", "Requests", 0.0, None);
        count.count = Some(400);
        for used in [f64::NAN, -1.0] {
            let now = at(0);
            let mut bad = reading("claude", now, vec![window("a", "A", used, None), count.clone()]);
            bad.snapshot.fetched_at = now;
            assert!(engine.observe(now, &settings, T, &[bad], &[]).events.is_empty());
        }
        let now = at(1);
        count.used = 1.0;
        let reading = reading("claude", now, vec![count]);
        assert!(engine.observe(now, &settings, T, &[reading], &[]).events.is_empty());
    }

    #[test]
    fn a_weekly_limit_answers_to_its_own_switch() {
        let weekly = vec![window("seven_day", "Weekly", 1.0, None)];
        let low = vec![window("seven_day", "Weekly", 0.1, None)];
        let run = |settings: &AlertSettings| {
            let mut engine = Engine::new(START);
            engine.observe(at(0), settings, T, &[reading("claude", at(0), low.clone())], &[]);
            let events = engine.observe(at(1), settings, T, &[reading("claude", at(1), weekly.clone())], &[]).events;
            events.iter().map(|event| event.kind).collect::<Vec<_>>()
        };
        assert_eq!(run(&all_on()), [AlertKind::LimitWeekly]);
        let session_only = AlertSettings { usage_limit_weekly: false, usage_critical: false, usage_watch: false, ..all_on() };
        assert!(run(&session_only).is_empty());
    }

    fn reset_step(engine: &mut Engine, now: u64, used: f64, resets_at: Option<u64>) -> Vec<AlertKind> {
        let windows = vec![window("five_hour", "5-hour", used, resets_at)];
        engine.observe(now, &all_on(), T, &[reading("claude", now, windows)], &[]).events.iter().map(|e| e.kind).collect()
    }

    #[test]
    fn a_reset_fires_only_after_the_old_window_elapsed() {
        let mut engine = Engine::new(START);
        let (t0, deadline) = (at(0), at(4));
        reset_step(&mut engine, t0, 0.4, Some(deadline));
        // Moved reset time and a lower reading before the deadline are corrections, not resets.
        assert!(reset_step(&mut engine, at(1), 0.15, Some(deadline + 5_000)).is_empty());
        assert!(reset_step(&mut engine, at(2), 0.4, Some(deadline)).is_empty());
        assert_eq!(reset_step(&mut engine, at(5), 0.01, Some(at(20))), [AlertKind::UsageReset]);
        assert!(reset_step(&mut engine, at(6), 0.01, Some(at(20))).is_empty());
    }

    #[test]
    fn little_prior_usage_is_not_worth_a_reset_alert() {
        let mut engine = Engine::new(START);
        reset_step(&mut engine, at(0), 0.05, Some(at(1)));
        assert!(reset_step(&mut engine, at(2), 0.0, Some(at(20))).is_empty());
    }

    #[test]
    fn a_provider_without_reset_times_resets_on_a_real_drop() {
        let mut engine = Engine::new(START);
        reset_step(&mut engine, at(0), 0.6, None);
        assert!(reset_step(&mut engine, at(1), 0.55, None).is_empty());
        assert_eq!(reset_step(&mut engine, at(2), 0.02, None), [AlertKind::UsageReset]);
    }

    #[test]
    fn one_alert_per_provider_per_reading_and_the_tick_is_capped() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        let many = |used: f64, now: u64| -> Vec<ProviderReading> {
            (0..8)
                .map(|n| {
                    reading(
                        &format!("p{n}"),
                        now,
                        vec![window("a", "Five-hour", used, None), window("b", "Weekly", used, None)],
                    )
                })
                .collect()
        };
        engine.observe(at(0), &settings, T, &many(0.1, at(0)), &[]);
        let observation = engine.observe(at(1), &settings, T, &many(1.0, at(1)), &[]);
        assert_eq!(observation.events.len(), MAX_ALERTS_PER_TICK);
        assert_eq!(observation.suppressed, 4);
        let subjects: std::collections::HashSet<_> = observation.events.iter().map(|e| e.subject.clone()).collect();
        assert_eq!(subjects.len(), MAX_ALERTS_PER_TICK);
    }

    #[test]
    fn nothing_rings_about_agents_on_start_or_for_a_session_already_finished() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        let first = [agent("a", AgentState::Attention), agent("b", AgentState::Done)];
        assert!(agents_step(&mut engine, 0, &settings, &first).is_empty());
        assert!(agents_step(&mut engine, 1, &settings, &first).is_empty());
        let late = [agent("a", AgentState::Attention), agent("b", AgentState::Done), agent("c", AgentState::Done)];
        assert!(agents_step(&mut engine, 2, &settings, &late).is_empty());
    }

    #[test]
    fn the_startup_grace_records_a_change_without_ringing() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        let now = START + 1_000;
        engine.observe(now, &settings, T, &[], &[agent("a", AgentState::Running)]);
        let replayed = engine.observe(now + 2_000, &settings, T, &[], &[agent("a", AgentState::Done)]);
        assert!(replayed.events.is_empty());
    }

    #[test]
    fn work_and_attention_each_fire_on_their_transition() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        agents_step(&mut engine, 0, &settings, &[agent("a", AgentState::Running)]);
        assert_eq!(agents_step(&mut engine, 1, &settings, &[agent("a", AgentState::Attention)]), [AlertKind::Attention]);
        assert!(agents_step(&mut engine, 2, &settings, &[agent("a", AgentState::Attention)]).is_empty());
        assert!(agents_step(&mut engine, 3, &settings, &[agent("a", AgentState::Running)]).is_empty());
        assert_eq!(agents_step(&mut engine, 4, &settings, &[agent("a", AgentState::Done)]), [AlertKind::WorkComplete]);
        assert!(agents_step(&mut engine, 5, &settings, &[agent("a", AgentState::Done)]).is_empty());
        // First seen waiting, after the baseline: it needs you whether or not this run saw it start.
        assert_eq!(
            agents_step(&mut engine, 6, &settings, &[agent("a", AgentState::Done), agent("b", AgentState::Attention)]),
            [AlertKind::Attention]
        );
    }

    #[test]
    fn a_flapping_session_is_held_back_by_its_cooldown() {
        let mut engine = Engine::new(START);
        let settings = all_on();
        let base = at(0);
        let attention = [agent("a", AgentState::Attention)];
        let running = [agent("a", AgentState::Running)];
        engine.observe(base, &settings, T, &[], &running);
        assert_eq!(engine.observe(base + 2_000, &settings, T, &[], &attention).events.len(), 1);
        engine.observe(base + 4_000, &settings, T, &[], &running);
        let held = engine.observe(base + 6_000, &settings, T, &[], &attention);
        assert!(held.events.is_empty());
        assert_eq!(held.suppressed, 1);
        engine.observe(base + AGENT_COOLDOWN_MS + 2_000, &settings, T, &[], &running);
        assert_eq!(engine.observe(base + AGENT_COOLDOWN_MS + 4_000, &settings, T, &[], &attention).events.len(), 1);
    }

    #[test]
    fn wording_escapes_markup_and_keeps_private_text_out() {
        let event = AlertEvent {
            kind: AlertKind::Attention,
            subject: "session:secret-id".into(),
            name: "a<b>&\"c'\u{7}".into(),
            window_id: String::new(),
            window_label: String::new(),
            used_percent: 0,
            resets_at: None,
        };
        let toast = toast_for(&event, &AlertSettings { sound: false, ..all_on() }, &|_| None);
        let xml = toast_xml(&toast);
        assert!(xml.contains("a&lt;b&gt;&amp;&quot;c&apos;</text>"));
        assert!(xml.contains("<audio silent=\"true\"/>"));
        assert!(!xml.contains("secret-id"));
        assert_eq!(toast.tag.len(), 32);
        assert!(toast_xml(&Toast { silent: false, ..toast }).find("<audio").is_none());
    }

    #[test]
    fn a_limit_names_its_reset_time_when_known() {
        let event = AlertEvent {
            kind: AlertKind::LimitWeekly,
            subject: "claude".into(),
            name: "Claude".into(),
            window_id: "w".into(),
            window_label: "Weekly".into(),
            used_percent: 100,
            resets_at: Some(5),
        };
        let toast = toast_for(&event, &all_on(), &|_| Some("14:05".into()));
        assert_eq!(toast.title, "Claude limit reached");
        assert_eq!(toast.body, "Its weekly limit is spent. Resets 14:05.");
        let none = toast_for(&event, &all_on(), &|_| None);
        assert_eq!(none.body, "Its weekly limit is spent.");
    }

    struct FakeSink {
        result: Result<(), String>,
        shown: Mutex<Vec<Toast>>,
    }

    impl NotificationSink for FakeSink {
        fn availability(&self) -> Availability {
            Availability::new("enabled", true, "fake")
        }
        fn show(&self, toast: &Toast) -> Result<(), String> {
            self.shown.lock().unwrap().push(toast.clone());
            self.result.clone()
        }
    }

    fn work_event() -> AlertEvent {
        AlertEvent {
            kind: AlertKind::WorkComplete,
            subject: "session:private".into(),
            name: "project · ab12".into(),
            window_id: String::new(),
            window_label: String::new(),
            used_percent: 0,
            resets_at: None,
        }
    }

    #[test]
    fn a_failure_is_counted_and_reported_never_shown_as_delivered() {
        let sink = FakeSink { result: Err("Windows refused".into()), shown: Mutex::new(Vec::new()) };
        let stats = Mutex::new(Stats::default());
        let result = deliver(&sink, &stats, &work_event(), &all_on(), &|_| None, 7);
        let stats = stats.into_inner().unwrap();
        assert_eq!(result.unwrap_err(), "Windows refused");
        assert_eq!((stats.delivered, stats.failed), (0, 1));
        let last = stats.last.unwrap();
        assert!(!last.ok);
        assert_eq!(last.error, "Windows refused");
    }

    #[test]
    fn a_delivery_reaches_the_sink_and_the_status_carries_no_session_identity() {
        let sink = FakeSink { result: Ok(()), shown: Mutex::new(Vec::new()) };
        let stats = Mutex::new(Stats::default());
        deliver(&sink, &stats, &work_event(), &all_on(), &|_| None, 7).unwrap();
        let stats = stats.into_inner().unwrap();
        assert_eq!(sink.shown.lock().unwrap().len(), 1);
        assert_eq!((stats.delivered, stats.failed), (1, 0));
        let last = stats.last.unwrap();
        assert!(last.ok);
        assert!(last.provider.is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn an_unsupported_platform_says_so() {
        let sink = UnsupportedSink;
        assert!(!sink.availability().can_deliver);
        assert!(sink.show(&toast_for(&work_event(), &all_on(), &|_| None)).is_err());
    }
}
