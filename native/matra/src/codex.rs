//! Codex usage adapter, implemented from the upstream Matra's documented behaviour.
//!
//! Live endpoint, native-client recovery, and a local rollout fallback:
//!   1. Live: borrow the session Codex keeps in `~/.codex/auth.json` (`tokens.access_token` +
//!      `tokens.account_id`) and GET `https://chatgpt.com/backend-api/wham/usage`. The reply carries
//!      `rate_limit.{primary_window,secondary_window}` with `used_percent / limit_window_seconds /
//!      reset_at (seconds) | reset_after_seconds`, plus a top-level `plan_type`. That is the number
//!      for *now*, and it starts no process. The token is read only — never refreshed, never written
//!      back; 401/403 becomes needsAuth and Codex renews it on its own.
//!   2. If the direct read fails (but not during a 429 backoff), a native codex.exe can read
//!      account/rateLimits/read using Codex's own authentication. No cmd/node wrapper is spawned;
//!      the owned process is hidden, bounded to 20 seconds, killed and reaped. The explicit
//!      `codex` bucket wins over the legacy single-bucket view, which can refer to Spark.
//!      This is recovery for a stored-token HTTP failure while the installed client can still
//!      authenticate, not the old unconditional cmd/node process tree removed in 1.5.0.
//!      Codex owns any managed OAuth refresh; Matra sends no login/refresh request itself.
//!   3. Fallback: Codex writes the limits it saw on each turn into the thread's rollout log
//!      `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`, as lines like
//!      `{"timestamp":"…","type":"event_msg","payload":{"type":"token_count","rate_limits":{
//!         "primary":{"used_percent":0.0,"window_minutes":300,"resets_at":1790585719},
//!         "secondary":{…}|null,"plan_type":"free"}}}`
//!      The reset is **resets_at, absolute seconds** (the documented resets_in_seconds is accepted
//!      too). This is the number from the *last run* — reading a file always succeeds instantly, so
//!      the reading is marked stale by the line's own timestamp (> 5 min).
//!      Like macOS, prefer the thread index in state_5.sqlite (read-only, WAL-aware, at most
//!      eight paths). If unavailable, retain the bounded three-date-directory scan. A resumed
//!      old thread keeps its creation directory, but the index records its latest activity.
//!
//! Credentials are borrowed, never managed: the numbers come from Codex's own sign-in and Codex's
//! own endpoint. No sign-in and no session history at all means absent (no cell is shown).

use crate::usage::{
    CodexDailyUsage, CodexDetails, CodexResetCredits, CodexStatistics, LimitWindow, UsageSnapshot,
};
use crate::AppState;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const POLL_SECS: u64 = 300; // Preserve the upstream cadence; a tray refresh interrupts it.
const TAIL_BYTES: u64 = 256 * 1024;
const CURRENT_FOR_MS: u64 = 5 * 60 * 1000;
const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const PROFILE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/profiles/me";
const CREDITS_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
const MAX_AUTH_BYTES: u64 = 64 * 1024;
const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;
const BACKOFF_MIN_SECS: u64 = 60; // wait at least this long after a 429; Retry-After only raises it

static REFRESH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Retry deadline given by the server (ms epoch): neither a manual refresh nor a restart may bypass it
static BACKOFF_UNTIL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// When the native client may be tried again after it came back with nothing.
///
/// `read_app_server` spawns `codex app-server` and waits on it. On a machine that has Codex
/// installed but is signed out, that call fails every time, and the poll runs every five minutes —
/// a hidden process, up to twenty seconds long, for as long as the app is open. The native read is
/// a fallback for managed sign-ins `auth.json` cannot describe, not something worth paying for on
/// every poll, so a failed attempt stands the path down for half an hour.
static NATIVE_RETRY_AFTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const NATIVE_STAND_DOWN_MS: u64 = 30 * 60 * 1000;

pub fn request_refresh() {
    REFRESH.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn codex_home() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codex"))
}

fn store_path() -> PathBuf {
    crate::config::config_path().with_file_name("codex.json")
}

pub fn load_persisted() -> UsageSnapshot {
    read_persisted_snapshot()
        .map(|mut s| {
            if !s.windows.is_empty() {
                s.status = "stale".into();
            }
            BACKOFF_UNTIL.store(s.backoff_until, std::sync::atomic::Ordering::Relaxed);
            let current = load_credential().map(|credential| account_key(&credential.account_id));
            snapshot_for_account(s, current.as_deref())
        })
        .unwrap_or_default()
}

fn read_persisted_snapshot() -> Option<UsageSnapshot> {
    let file = std::fs::File::open(store_path()).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn account_key(account_id: &str) -> String {
    format!("{:x}", Sha256::digest(account_id.as_bytes()))
}

fn details_for_account(
    details: Option<CodexDetails>,
    current: Option<&str>,
) -> Option<CodexDetails> {
    details.filter(|details| {
        current.is_some_and(|current| !current.is_empty() && details.account_key == current)
    })
}

fn revalidate_details(mut snapshot: UsageSnapshot) -> UsageSnapshot {
    let key = load_credential().map(|credential| account_key(&credential.account_id));
    let had_details = snapshot.codex_details.is_some();
    snapshot = snapshot_for_account(snapshot, key.as_deref());
    if had_details && snapshot.codex_details.is_none() {
        request_refresh();
    }
    snapshot
}

fn snapshot_for_account(mut snapshot: UsageSnapshot, current: Option<&str>) -> UsageSnapshot {
    let had_details = snapshot.codex_details.is_some();
    snapshot.codex_details = details_for_account(snapshot.codex_details, current);
    if had_details && snapshot.codex_details.is_none() {
        // The quota and details were fetched together. Once that account is
        // known to have changed, neither may be presented as the new account.
        return UsageSnapshot {
            status: "none".into(),
            note: "Codex account changed — refreshing".into(),
            backoff_until: snapshot.backoff_until,
            ..Default::default()
        };
    }
    snapshot
}

fn plan_label(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| {
            !value.is_empty() && value.len() <= 80 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned)
}

fn persist(s: &UsageSnapshot) {
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(store_path(), t);
    }
}

// ---------------- Locating the executable ----------------

/// Candidates in order: the native exe inside the global npm package (cleanest — no cmd/node
/// wrapper) → ~/.codex/bin → codex.exe / codex.cmd on PATH.
pub fn find_executable() -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = Vec::new();
    if let Some(appdata) = dirs::config_dir() {
        let pkg = appdata.join("npm").join("node_modules").join("@openai").join("codex");
        if let Ok(rd) = std::fs::read_dir(pkg.join("bin")) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().to_lowercase();
                if n.starts_with("codex-") && n.contains("windows") && n.ends_with(".exe") {
                    cands.push(e.path());
                }
            }
        }
        if let Ok(rd) = std::fs::read_dir(pkg.join("vendor")) {
            // Newer packages keep the native exe at vendor/<triple>/codex/codex.exe
            for e in rd.flatten() {
                let p = e.path().join("codex").join("codex.exe");
                if p.exists() {
                    cands.push(p);
                }
            }
        }
        cands.push(appdata.join("npm").join("codex.cmd"));
    }
    if let Some(h) = codex_home() {
        cands.push(h.join("bin").join("codex.exe"));
        cands.push(h.join("bin").join("codex"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            cands.push(dir.join("codex.exe"));
            cands.push(dir.join("codex.cmd"));
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

// ---------------- Live: the usage endpoint ----------------

fn auth_path() -> Option<PathBuf> {
    codex_home().map(|h| h.join("auth.json"))
}

struct Credential {
    access_token: String,
    account_id: String,
    /// chatgpt_plan_type from the id_token (pro / plus / free…), used only as a label
    plan: Option<String>,
    /// The access_token's exp has passed: the request is still sent (the server decides); this only changes the 401 wording
    expired: bool,
}

/// Second JWT segment (base64url) → claims. Used only for labels and a local expiry hint; nothing is verified here — that is the server's job
fn jwt_claims(token: &str) -> Option<serde_json::Value> {
    let part = token.split('.').nth(1)?;
    let raw = crate::antigravity::b64_decode(part)?;
    serde_json::from_slice(&raw).ok()
}

/// Reads Codex's sign-in state; a missing file or missing field both mean "not signed in"
fn load_credential() -> Option<Credential> {
    let file = std::fs::File::open(auth_path()?).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_AUTH_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_AUTH_BYTES {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let tokens = v.get("tokens")?;
    let access_token = tokens.get("access_token")?.as_str()?.trim().to_string();
    let account_id = tokens.get("account_id")?.as_str()?.trim().to_string();
    if access_token.is_empty()
        || account_id.is_empty()
        || access_token.len() > 32 * 1024
        || account_id.len() > 256
        || !access_token
            .bytes()
            .chain(account_id.bytes())
            .all(|byte| (33..=126).contains(&byte))
    {
        return None;
    }
    let expired = jwt_claims(&access_token)
        .and_then(|c| c.get("exp").and_then(|x| x.as_f64()))
        .map(|exp| exp * 1000.0 <= now_ms() as f64)
        .unwrap_or(false);
    let plan = tokens
        .get("id_token")
        .and_then(|x| x.as_str())
        .and_then(jwt_claims)
        .and_then(|c| {
            c.get("https://api.openai.com/auth")?
                .get("chatgpt_plan_type")?
                .as_str()
                .and_then(|plan| plan_label(Some(plan)))
        });
    Some(Credential {
        access_token,
        account_id,
        plan,
        expired,
    })
}

enum LiveErr {
    NeedsAuth,
    /// Suggested wait in seconds (BACKOFF_MIN_SECS already applied)
    RateLimited(u64),
    Other(String),
}

fn fetch_usage(cred: &Credential) -> Result<serde_json::Value, LiveErr> {
    fetch_json(cred, ENDPOINT, false)
}

fn fetch_json(
    cred: &Credential,
    endpoint: &'static str,
    credits: bool,
) -> Result<serde_json::Value, LiveErr> {
    // Fixed endpoints use OS trust without redirects. Authentication must never
    // be forwarded to a location supplied by a remote redirect.
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    let agent = AGENT.get_or_init(|| crate::http::builder().redirects(0).build());
    let request = agent
        .get(endpoint)
        .set("Authorization", &format!("Bearer {}", cred.access_token))
        .set("ChatGPT-Account-Id", &cred.account_id)
        .set("Accept", "application/json")
        .set("Cache-Control", "no-cache, no-store")
        .set(
            "User-Agent",
            concat!("matra/", env!("CARGO_PKG_VERSION"), " (Windows)"),
        )
        .timeout(Duration::from_secs(15));
    let request = if credits {
        request.set("OpenAI-Beta", "codex-1")
    } else {
        request
    };
    let resp = request.call();
    match resp {
        Ok(r) => {
            let mut bytes = Vec::new();
            r.into_reader()
                .take(MAX_RESPONSE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| LiveErr::Other("response unavailable".into()))?;
            if bytes.len() as u64 > MAX_RESPONSE_BYTES {
                return Err(LiveErr::Other("response too large".into()));
            }
            serde_json::from_slice(&bytes).map_err(|_| LiveErr::Other("unreadable response".into()))
        }
        Err(ureq::Error::Status(code @ (401 | 403), _)) => {
            crate::applog(&format!("codex: endpoint HTTP {code}"));
            Err(LiveErr::NeedsAuth)
        }
        Err(ureq::Error::Status(429, r)) => {
            let ra = retry_after_secs(r.header("retry-after"), now_ms());
            Err(LiveErr::RateLimited(ra.max(BACKOFF_MIN_SECS)))
        }
        Err(ureq::Error::Status(code, _)) => Err(LiveErr::Other(format!("HTTP {code}"))),
        Err(_) => Err(LiveErr::Other("request failed".into())),
    }
}

fn retry_after_secs(header: Option<&str>, now: u64) -> u64 {
    let Some(header) = header.map(str::trim) else {
        return 0;
    };
    if let Ok(seconds) = header.parse::<u64>() {
        return seconds;
    }
    chrono::DateTime::parse_from_rfc2822(header)
        .ok()
        .and_then(|date| u64::try_from(date.timestamp_millis()).ok())
        .map(|deadline| deadline.saturating_sub(now).div_ceil(1000))
        .unwrap_or(0)
}

fn hold_backoff(seconds: u64, now: u64) -> u64 {
    let deadline = now.saturating_add(seconds.max(BACKOFF_MIN_SECS).saturating_mul(1000));
    BACKOFF_UNTIL.store(deadline, std::sync::atomic::Ordering::Relaxed);
    deadline
}

fn parse_statistics(value: &serde_json::Value) -> Result<CodexStatistics, LiveErr> {
    let stats = value
        .get("stats")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| LiveErr::Other("unreadable profile statistics".into()))?;
    let count = |key: &str| stats.get(key).and_then(serde_json::Value::as_u64);
    let mut statistics = CodexStatistics {
        lifetime_tokens: count("lifetime_tokens"),
        peak_daily_tokens: count("peak_daily_tokens"),
        longest_running_turn_sec: stats
            .get("longest_running_turn_sec")
            .and_then(serde_json::Value::as_f64)
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0),
        current_streak_days: count("current_streak_days"),
        longest_streak_days: count("longest_streak_days"),
        daily_usage_buckets: None,
    };
    if let Some(days) = stats
        .get("daily_usage_buckets")
        .and_then(serde_json::Value::as_array)
    {
        if days.len() > 4000 {
            return Err(LiveErr::Other("profile history too large".into()));
        }
        let mut unique = BTreeMap::new();
        let mut complete = true;
        for day in days {
            let date = day.get("start_date").and_then(serde_json::Value::as_str);
            let tokens = day.get("tokens").and_then(serde_json::Value::as_u64);
            match (date, tokens) {
                (Some(date), Some(tokens)) if valid_day(date) => {
                    unique.insert(date.to_owned(), tokens);
                }
                _ => complete = false,
            }
        }
        if complete {
            statistics.daily_usage_buckets = Some(
                unique
                    .into_iter()
                    .map(|(start_date, tokens)| CodexDailyUsage { start_date, tokens })
                    .collect(),
            );
        }
    }
    Ok(statistics)
}

fn valid_day(date: &str) -> bool {
    date.len() == 10
        && date.as_bytes().iter().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
        && chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok()
}

fn parse_credits(value: &serde_json::Value, now: u64) -> Result<CodexResetCredits, LiveErr> {
    let object = value
        .as_object()
        .ok_or_else(|| LiveErr::Other("unreadable reset credits".into()))?;
    let count = object
        .get("available_count")
        .and_then(serde_json::Value::as_u64);
    let listed = object.get("credits").and_then(serde_json::Value::as_array);
    if count.is_none() && listed.is_none()
        || object
            .get("available_count")
            .is_some_and(|value| !value.is_null() && value.as_u64().is_none())
    {
        return Err(LiveErr::Other("unreadable reset credits".into()));
    }
    let mut listed_count = 0;
    let mut next_expiry_ms = None;
    if let Some(listed) = listed {
        if listed.len() > 4096 {
            return Err(LiveErr::Other("reset credit list too large".into()));
        }
        for credit in listed {
            let status = credit
                .as_object()
                .and_then(|credit| credit.get("status"))
                .and_then(serde_json::Value::as_str);
            if count.is_none()
                && !matches!(
                    status,
                    Some("available" | "redeemed" | "used" | "consumed" | "spent" | "expired")
                )
            {
                return Err(LiveErr::Other("unreadable reset credits".into()));
            }
            if status != Some("available") {
                continue;
            }
            listed_count += 1;
            if let Some(expiry) = credit
                .get("expires_at")
                .and_then(serde_json::Value::as_str)
                .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                .and_then(|date| u64::try_from(date.timestamp_millis()).ok())
                .filter(|expiry| *expiry > now)
            {
                next_expiry_ms =
                    Some(next_expiry_ms.map_or(expiry, |previous: u64| previous.min(expiry)));
            }
        }
    }
    let available_count = count.unwrap_or(listed_count);
    Ok(CodexResetCredits {
        available_count,
        next_expiry_ms: if available_count > 0 {
            next_expiry_ms
        } else {
            None
        },
        fetched_at: now,
    })
}

#[derive(Clone, Copy)]
enum DetailEndpoint {
    Profile,
    Credits,
}

fn fetch_details(
    credential: &Credential,
    plan: Option<String>,
    cached: Option<CodexDetails>,
) -> (CodexDetails, u64) {
    fetch_details_with(
        credential,
        plan,
        cached,
        |endpoint| match endpoint {
            DetailEndpoint::Profile => fetch_json(credential, PROFILE_ENDPOINT, false),
            DetailEndpoint::Credits => fetch_json(credential, CREDITS_ENDPOINT, true),
        },
        now_ms,
    )
}

fn fetch_details_with(
    credential: &Credential,
    plan: Option<String>,
    cached: Option<CodexDetails>,
    mut get: impl FnMut(DetailEndpoint) -> Result<serde_json::Value, LiveErr>,
    mut clock: impl FnMut() -> u64,
) -> (CodexDetails, u64) {
    let now = clock();
    let key = account_key(&credential.account_id);
    let mut details = details_for_account(cached, Some(&key)).unwrap_or_else(|| CodexDetails {
        account_key: key,
        fetched_at: now,
        ..Default::default()
    });
    details.plan = plan;
    details.statistics_error = None;
    details.credits_error = None;
    let mut backoff = 0;
    for endpoint in [DetailEndpoint::Profile, DetailEndpoint::Credits] {
        let response = get(endpoint);
        // Retry-After starts when the failing request finishes, not when the
        // preceding optional request began. Cached readings keep their dates.
        let received_at = clock();
        let result = response.and_then(|value| match endpoint {
            DetailEndpoint::Profile => {
                details.statistics = Some(parse_statistics(&value)?);
                details.fetched_at = received_at;
                Ok(())
            }
            DetailEndpoint::Credits => {
                details.credits = Some(parse_credits(&value, received_at)?);
                Ok(())
            }
        });
        if let Err(error) = result {
            let message = match &error {
                LiveErr::NeedsAuth => "Sign-in unavailable",
                LiveErr::RateLimited(_) => "Rate limited",
                LiveErr::Other(_) => "Reading unavailable",
            }
            .to_owned();
            match endpoint {
                DetailEndpoint::Profile => details.statistics_error = Some(message),
                DetailEndpoint::Credits => details.credits_error = Some(message),
            }
            if let LiveErr::RateLimited(seconds) = error {
                backoff =
                    received_at.saturating_add(seconds.max(BACKOFF_MIN_SECS).saturating_mul(1000));
                if matches!(endpoint, DetailEndpoint::Profile) {
                    details.credits_error = Some("Waiting for retry".into());
                }
                break;
            }
        }
    }
    (details, backoff)
}

/// Upstream's label rule: Codex names windows only by length, and "5h limit" says more than "primary"
fn label_for(window_minutes: Option<f64>, id: &str) -> String {
    match window_minutes {
        Some(m) if m > 0.0 => {
            if m < 60.0 {
                format!("{}m limit", m as i64)
            } else if m < 60.0 * 24.0 {
                format!("{}h limit", (m / 60.0) as i64)
            } else {
                let days = (m / (60.0 * 24.0)).round() as i64;
                match days {
                    7 => "Weekly limit".into(),
                    30 => "Monthly limit".into(),
                    d => format!("{d}d limit"),
                }
            }
        }
        _ => {
            if id == "primary" {
                "Current session".into()
            } else {
                "Longer window".into()
            }
        }
    }
}

fn num(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64())
}

/// Seconds → ms. Negative / non-finite values are treated as missing so a
/// garbage extra cannot wrap `now + ms` (debug overflow panics).
fn secs_to_ms(s: f64) -> Option<u64> {
    if !s.is_finite() || s < 0.0 {
        None
    } else {
        Some((s * 1000.0) as u64)
    }
}

fn reset_at_ms(w: &serde_json::Value, now: u64, epoch_key: &str, delay_key: &str) -> Option<u64> {
    num(w.get(epoch_key))
        .and_then(secs_to_ms)
        .or_else(|| num(w.get(delay_key)).and_then(secs_to_ms).map(|ms| now.saturating_add(ms)))
}

/// Skip a window with no `used_percent`. Extra ids still pass primary/secondary to `label_for`.
fn window_from(
    w: &serde_json::Value,
    id: &str,
    fallback: &str,
    now: u64,
    group: Option<&str>,
) -> Option<LimitWindow> {
    if !w.is_object() {
        return None;
    }
    let pct = num(w.get("used_percent"))?;
    Some(LimitWindow {
        id: id.into(),
        label: label_for(num(w.get("limit_window_seconds")).map(|s| s / 60.0), fallback),
        used: (pct / 100.0).clamp(0.0, 1.0),
        resets_at: reset_at_ms(w, now, "reset_at", "reset_after_seconds"),
        group: group.map(str::to_string),
        ..Default::default()
    })
}

fn names_spark(extra: &serde_json::Value) -> bool {
    if !extra.is_object() {
        return false;
    }
    ["limit_name", "metered_feature"].iter().any(|key| {
        extra
            .get(*key)
            .and_then(|x| x.as_str())
            .is_some_and(|s| s.to_lowercase().contains("spark"))
    })
}

/// Spark / code review sit after the main pair and remain separate detail rows.
/// The group is what the hover card uses to box them; omitting it leaves them
/// as extra ungrouped bars under the main windows.
fn append_extra(
    rl: Option<&serde_json::Value>,
    primary_id: &str,
    secondary_id: &str,
    group: &str,
    now: u64,
    out: &mut Vec<LimitWindow>,
) {
    let Some(rl) = rl.filter(|x| x.is_object()) else {
        return;
    };
    if let Some(w) = rl
        .get("primary_window")
        .and_then(|x| window_from(x, primary_id, "primary", now, Some(group)))
    {
        push_unique(out, w);
    }
    if let Some(w) = rl
        .get("secondary_window")
        .and_then(|x| window_from(x, secondary_id, "secondary", now, Some(group)))
    {
        push_unique(out, w);
    }
}

fn push_unique(out: &mut Vec<LimitWindow>, window: LimitWindow) {
    if out.iter().any(|w| w.id == window.id) {
        return;
    }
    out.push(window);
}

/// Usage reply → windows. Primary and secondary feed the ring; Spark
/// (`additional_rate_limits`) and Code review (`code_review_rate_limit`) belong
/// on the hover card, not as extra rings. The window id records which field it
/// came from and the label is derived from the length — the primary window is
/// not always five hours (a free plan has shown 30 days), and recognising only
/// fixed lengths would drop a window that is genuinely in use.
fn windows_from_usage(v: &serde_json::Value) -> Vec<LimitWindow> {
    let now = now_ms();
    let mut out = Vec::new();
    for (id, key) in [("primary", "primary_window"), ("secondary", "secondary_window")] {
        if let Some(w) = v
            .pointer(&format!("/rate_limit/{key}"))
            .and_then(|x| window_from(x, id, id, now, None))
        {
            out.push(w);
        }
    }
    // A non-array (null, object, string) is the same as omitting the field —
    // one junk extra must not discard the main pair or a later Spark row.
    if let Some(extras) = v.get("additional_rate_limits").and_then(|x| x.as_array()) {
        for extra in extras {
            if !names_spark(extra) {
                continue;
            }
            append_extra(extra.get("rate_limit"), "spark", "spark-secondary", "Spark", now, &mut out);
        }
    }
    append_extra(
        v.get("code_review_rate_limit"),
        "code-review",
        "code-review-secondary",
        "Code review",
        now,
        &mut out,
    );
    out
}

// ---------------- Fallback: the rollout snapshot ----------------

/// Creation dates do not indicate activity: resumed threads keep their original directory.
pub fn newest_rollout() -> Option<PathBuf> {
    newest_rollout_in(&codex_home()?)
}

fn newest_rollout_in(home: &Path) -> Option<PathBuf> {
    indexed_rollout(&home.join("state_5.sqlite"))
        .or_else(|| newest_recent_rollout(&home.join("sessions")))
}

fn indexed_rollout(database: &Path) -> Option<PathBuf> {
    use rusqlite::{Connection, OpenFlags};
    // No immutable=1: resumed-thread updates may still be in the writer's WAL.
    // Never create/migrate the database; schema changes or contention use the bounded fallback.
    let db = Connection::open_with_flags(
        database, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ).ok()?;
    db.busy_timeout(Duration::from_millis(50)).ok()?;
    let mut query = db.prepare(
        "SELECT rollout_path FROM threads WHERE archived = 0 ORDER BY updated_at_ms DESC LIMIT 8",
    ).or_else(|_| db.prepare(
        "SELECT rollout_path FROM threads WHERE archived = 0 ORDER BY updated_at DESC LIMIT 8",
    )).ok()?;
    let paths = query.query_map([], |row| row.get::<_, String>(0)).ok()?;
    let found = paths.filter_map(Result::ok).map(PathBuf::from).find(|path| {
        path.file_name().and_then(|name| name.to_str())
            .map(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
            .unwrap_or(false) && path.is_file()
    });
    found
}

fn newest_recent_rollout(root: &Path) -> Option<PathBuf> {
    let mut days: Vec<PathBuf> = Vec::new();
    let mut years = list_dirs(root);
    years.sort_by(|a, b| b.cmp(a));
    'outer: for y in years {
        let mut months = list_dirs(&y);
        months.sort_by(|a, b| b.cmp(a));
        for m in months {
            let mut ds = list_dirs(&m);
            ds.sort_by(|a, b| b.cmp(a));
            for d in ds {
                days.push(d);
                if days.len() >= 3 {
                    break 'outer;
                }
            }
        }
    }
    let mut best: Option<(SystemTime, PathBuf)> = None;
    for d in days {
        if let Ok(rd) = std::fs::read_dir(&d) {
            for e in rd.flatten() {
                let p = e.path();
                let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                if !(name.starts_with("rollout-") && name.ends_with(".jsonl")) {
                    continue;
                }
                let Ok(md) = e.metadata() else { continue };
                let Ok(mt) = md.modified() else { continue };
                if best.as_ref().map(|(t, _)| mt > *t).unwrap_or(true) {
                    best = Some((mt, p));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

fn list_dirs(p: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default()
}

pub fn tail_text(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = f.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)));
    let mut raw = Vec::new();
    f.read_to_end(&mut raw).ok()?;
    Some(String::from_utf8_lossy(&raw).into_owned())
}

/// The last rate_limits snapshot at the tail of a rollout → (windows, recorded-at ms, plan)
pub fn snapshot_from_rollout(text: &str) -> Option<(Vec<LimitWindow>, Option<u64>, Option<String>)> {
    for line in text.lines().rev().filter(|l| l.contains("rate_limits")) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        // rate_limits may sit at the top level or under payload
        let rl = v
            .get("rate_limits")
            .or_else(|| v.pointer("/payload/rate_limits"))
            .filter(|x| x.is_object());
        let Some(rl) = rl else { continue };
        // Multiple buckets are emitted separately. Spark must never stand in for core Codex.
        // Legacy snapshots without an id are still accepted.
        if rl.get("limit_id").or_else(|| rl.get("limitId"))
            .and_then(|v| v.as_str()).map(|id| id != "codex").unwrap_or(false) {
            continue;
        }
        let recorded = v
            .get("timestamp")
            .and_then(|x| x.as_str())
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.timestamp_millis().max(0) as u64);
        let now = now_ms();
        let mut out = Vec::new();
        for id in ["primary", "secondary"] {
            let Some(w) = rl.get(id).filter(|x| x.is_object()) else { continue };
            let Some(pct) = num(w.get("used_percent")) else { continue };
            out.push(LimitWindow {
                id: id.into(),
                label: label_for(num(w.get("window_minutes")), id),
                used: (pct / 100.0).clamp(0.0, 1.0),
                resets_at: reset_at_ms(w, now, "resets_at", "resets_in_seconds"),
                ..Default::default()
            });
        }
        if out.is_empty() {
            continue;
        }
        let plan = rl.get("plan_type").and_then(|x| x.as_str()).map(String::from);
        return Some((out, recorded, plan));
    }
    None
}

// ---------------- Putting it together ----------------

/// Prefer the installed native Codex client: it understands the desktop's managed sign-in.
/// Only initialize + account/rateLimits/read are sent; no login or inference commands.
fn native_codex() -> Option<PathBuf> {
    if let Some(local) = dirs::data_local_dir() {
        let mut bins = list_dirs(&local.join("OpenAI/Codex/bin"));
        bins.sort_by_key(|p| std::cmp::Reverse(std::fs::metadata(p).and_then(|m| m.modified()).ok()));
        if let Some(exe) = bins.into_iter().map(|p| p.join("codex.exe")).find(|p| p.is_file()) {
            return Some(exe);
        }
    }
    find_executable().filter(|p| p.extension().and_then(|x| x.to_str()) == Some("exe"))
}

fn app_server_snapshot(result: &serde_json::Value) -> Option<UsageSnapshot> {
    // A present multi-bucket map is authoritative: never substitute a legacy Spark bucket
    // (or a legacy bucket with no id) when the map does not contain core Codex.
    let core = match result.get("rateLimitsByLimitId").filter(|v| !v.is_null()) {
        Some(buckets) => buckets.get("codex")?,
        None => result.get("rateLimits")?,
    };
    if core.get("limitId").and_then(|x| x.as_str()).map(|id| id != "codex").unwrap_or(false) {
        return None;
    }
    let mut windows = Vec::new();
    for id in ["primary", "secondary"] {
        let Some(w) = core.get(id).filter(|v| v.is_object()) else { continue };
        let Some(used) = w.get("usedPercent").and_then(|x| x.as_f64()) else { continue };
        windows.push(LimitWindow {
            id: id.into(),
            label: label_for(w.get("windowDurationMins").and_then(|x| x.as_f64()), id),
            used: (used / 100.0).clamp(0.0, 1.0),
            resets_at: w.get("resetsAt").and_then(|x| x.as_u64()).map(|s| s.saturating_mul(1000)),
            ..Default::default()
        });
    }
    if windows.is_empty() { return None; }
    Some(UsageSnapshot { status: "ok".into(), windows, fetched_at: now_ms(),
        note: "via Codex app-server".into(), ..Default::default() })
}

fn read_app_server() -> Option<UsageSnapshot> {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    let mut command = Command::new(native_codex()?);
    command.arg("app-server").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; command.creation_flags(0x0800_0000); }
    let mut child = command.spawn().ok()?;
    let result = (|| {
        let mut input = child.stdin.take()?;
        let output = child.stdout.take()?;
        let (tx, rx) = std::sync::mpsc::sync_channel(16);
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines().map_while(Result::ok) {
                if tx.send(line).is_err() { break; }
            }
        });
        writeln!(input, "{}", serde_json::json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"matra","version":env!("CARGO_PKG_VERSION")}}})).ok()?;
        input.flush().ok()?;
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let line = rx.recv_timeout(deadline.checked_duration_since(std::time::Instant::now())?).ok()?;
            let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
            match v.get("id").and_then(|x| x.as_u64()) {
                Some(1) => {
                    v.get("result")?;
                    writeln!(input, "{}", serde_json::json!({"method":"initialized","params":{}})).ok()?;
                    writeln!(input, "{}", serde_json::json!({"id":2,"method":"account/rateLimits/read"})).ok()?;
                    input.flush().ok()?;
                }
                Some(2) => return app_server_snapshot(v.get("result")?),
                _ => {}
            }
        }
    })();
    // This is a directly launched native executable, never a cmd/node wrapper or a running app.
    let _ = child.kill();
    let _ = child.wait();
    result
}

/// Is Codex present on this machine (CLI installed, signed in, or has had sessions)? If not, no cell is shown
pub fn present() -> bool {
    native_codex().is_some()
        || find_executable().is_some()
        || auth_path().map(|p| p.is_file()).unwrap_or(false)
        || codex_home().map(|h| h.join("sessions").is_dir()).unwrap_or(false)
}

fn read_once() -> UsageSnapshot {
    let mut snap = UsageSnapshot::default();
    let credential = load_credential();
    let current_key = credential
        .as_ref()
        .map(|credential| account_key(&credential.account_id));
    let cached_details = details_for_account(
        read_persisted_snapshot().and_then(|snapshot| snapshot.codex_details),
        current_key.as_deref(),
    );
    snap.codex_details = cached_details.clone();
    // Note attached to the fallback reading when the live read failed; needs_auth picks the empty state when there is no fallback either
    let mut live_note: Option<String> = None;
    let mut needs_auth = false;
    let held_until = BACKOFF_UNTIL.load(std::sync::atomic::Ordering::Relaxed);
    let now = now_ms();
    if held_until > now {
        snap.backoff_until = held_until;
        live_note = Some(format!(
            "Rate limited — retrying in {}s",
            (held_until - now) / 1000
        ));
    } else {
        match credential {
            None => {
                if auth_path().map(|p| p.is_file()).unwrap_or(false) {
                    crate::applog("codex: auth.json has no usable access_token/account_id, falling back to the rollout");
                }
            }
            Some(cred) => match fetch_usage(&cred) {
                Ok(v) => {
                    let windows = windows_from_usage(&v);
                    if !windows.is_empty() {
                        let plan = plan_label(v.get("plan_type").and_then(|x| x.as_str()))
                            .or_else(|| cred.plan.clone());
                        let (details, backoff) = fetch_details(&cred, plan.clone(), cached_details);
                        // Persist the server's hold even if the account changed
                        // while a request was in flight.
                        if backoff > 0 {
                            BACKOFF_UNTIL.store(backoff, std::sync::atomic::Ordering::Relaxed);
                            snap.backoff_until = backoff;
                        }
                        let key_after =
                            load_credential().map(|credential| account_key(&credential.account_id));
                        if key_after.as_deref() != Some(details.account_key.as_str()) {
                            // Authentication changed while the two optional requests
                            // were in flight. Do not publish the former account.
                            request_refresh();
                            return UsageSnapshot {
                                status: "none".into(),
                                note: "Codex account changed — refreshing".into(),
                                backoff_until: snap.backoff_until,
                                ..Default::default()
                            };
                        }
                        snap.status = "ok".into();
                        snap.windows = windows;
                        snap.fetched_at = now_ms();
                        snap.note = plan
                            .map(|p| format!("{} · via Codex", cap(&p)))
                            .unwrap_or_default();
                        snap.codex_details = Some(details);
                        return revalidate_details(snap);
                    }
                    crate::applog("codex: usage reply has no windows, falling back to the rollout");
                    live_note = Some("Codex reported no usage windows".into());
                }
                Err(LiveErr::NeedsAuth) => {
                    needs_auth = true;
                    live_note = Some(if cred.expired {
                        "Codex sign-in expired — open Codex once to refresh it".into()
                    } else {
                        "Codex rejected its sign-in — sign in to Codex again".into()
                    });
                }
                Err(LiveErr::RateLimited(secs)) => {
                    let until = hold_backoff(secs, now_ms());
                    snap.backoff_until = until;
                    live_note = Some(format!("Rate limited — retrying in {secs}s"));
                    crate::applog(&format!(
                        "codex: usage endpoint returned 429, retrying in {secs}s"
                    ));
                }
                Err(LiveErr::Other(e)) => {
                    crate::applog(&format!(
                        "codex: live read failed ({e}), falling back to the rollout"
                    ));
                    live_note = Some(format!("Live read failed ({e})"));
                }
            },
        }
    }
    // Keep the no-process HTTP path first, and do not use a second transport to bypass
    // its Retry-After. The native client can handle managed sign-in that auth.json cannot.
    if snap.backoff_until <= now_ms()
        && NATIVE_RETRY_AFTER.load(std::sync::atomic::Ordering::Relaxed) <= now_ms()
    {
        match read_app_server() {
            Some(mut native) => {
                // The managed native client does not publish its account ID in
                // this response. Do not attach an unrelated stored-token card.
                native.codex_details = None;
                return native;
            }
            None => NATIVE_RETRY_AFTER.store(
                now_ms() + NATIVE_STAND_DOWN_MS,
                std::sync::atomic::Ordering::Relaxed,
            ),
        }
    }
    // Fallback: rollout
    match newest_rollout()
        .and_then(|p| tail_text(&p))
        .and_then(|t| snapshot_from_rollout(&t))
    {
        Some((windows, recorded, plan)) => {
            let rec = recorded.unwrap_or(0);
            let fresh = rec > 0 && now_ms().saturating_sub(rec) <= CURRENT_FOR_MS;
            snap.status = if fresh { "ok" } else { "stale" }.into();
            snap.windows = windows;
            snap.fetched_at = rec; // the recorded time is what counts; the UI shows Updated N ago from it
            snap.note = match plan {
                Some(p) => format!("{} · from last Codex run", cap(&p)),
                None => "from last Codex run".into(),
            };
            if let Some(n) = live_note {
                snap.note = format!("{n} · {}", snap.note);
            }
        }
        None => {
            snap.status = if needs_auth {
                "needsAuth"
            } else if present() {
                "none"
            } else {
                "absent"
            }
            .into();
            snap.note = match live_note {
                Some(n) => n,
                None if present() => "Codex has not recorded a usage snapshot yet".into(),
                None => String::new(),
            };
        }
    }
    revalidate_details(snap)
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn broadcast(app: &AppHandle, snap: UsageSnapshot) {
    let snap = revalidate_details(snap);
    let st = app.state::<AppState>();
    *st.codex.lock().unwrap() = snap.clone();
    persist(&snap);
    let _ = app.emit("codex", &snap);
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        {
            let st = app.state::<AppState>();
            let snap = revalidate_details(st.codex.lock().unwrap().clone());
            let _ = app.emit("codex", &snap);
        }
        if !present() {
            broadcast(
                &app,
                UsageSnapshot {
                    status: "absent".into(),
                    ..Default::default()
                },
            );
            // Codex is not installed: look again every 10 minutes
            loop {
                for _ in 0..600 {
                    if REFRESH.swap(false, std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
                if present() {
                    break;
                }
            }
        }
        loop {
            let snap = read_once();
            let hold = snap.backoff_until.saturating_sub(now_ms()) / 1000;
            broadcast(&app, snap);
            for _ in 0..POLL_SECS.max(hold) {
                if REFRESH.swap(false, std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    });
}

/// For doctor: contains no secrets
pub fn probe() -> String {
    let auth = match load_credential() {
        Some(c) => format!(
            "auth.json usable{}{}",
            if c.expired {
                " (access_token expired)"
            } else {
                ""
            },
            c.plan.map(|p| format!(", plan={p}")).unwrap_or_default()
        ),
        None if auth_path().map(|p| p.is_file()).unwrap_or(false) => {
            "auth.json present but has no token".to_string()
        }
        None => "auth.json not found".to_string(),
    };
    let exe = find_executable();
    let roll = newest_rollout();
    let age = roll
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .map(|d| format!("{} min ago", d.as_secs() / 60))
        .unwrap_or_else(|| "?".into());
    format!(
        "Codex: {auth} | executable {} | newest rollout {} (modified {})",
        exe.map(|p| p.display().to_string())
            .unwrap_or_else(|| "not found".into()),
        roll.map(|p| p.display().to_string())
            .unwrap_or_else(|| "none".into()),
        age
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_credential(account: &str) -> Credential {
        Credential {
            access_token: "synthetic-test-token".into(),
            account_id: account.into(),
            plan: None,
            expired: false,
        }
    }

    fn cached_details(account: &str) -> CodexDetails {
        CodexDetails {
            account_key: account_key(account),
            fetched_at: 100,
            statistics: Some(CodexStatistics {
                lifetime_tokens: Some(7),
                ..Default::default()
            }),
            credits: Some(CodexResetCredits {
                available_count: 2,
                next_expiry_ms: Some(900_000),
                fetched_at: 200,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn old_usage_cache_loads_without_codex_details() {
        let old = r#"{"status":"ok","windows":[],"fetched_at":123,"note":"via Codex","backoff_until":456}"#;
        let snapshot: UsageSnapshot = serde_json::from_str(old).unwrap();
        assert!(snapshot.codex_details.is_none());
        assert_eq!(snapshot.backoff_until, 456);
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("codex_details"));
    }

    #[test]
    fn detail_cache_is_scoped_to_current_account_only() {
        let details = cached_details("account-a");
        assert_ne!(details.account_key, "account-a");
        assert_eq!(details.account_key.len(), 64);
        assert!(
            details_for_account(Some(details.clone()), Some(&account_key("account-a"))).is_some()
        );
        for current in [None, Some(""), Some("account-a"), Some("other-key")] {
            assert!(details_for_account(Some(details.clone()), current).is_none());
        }
        assert!(details_for_account(Some(details), Some(&account_key("account-b"))).is_none());
    }

    #[test]
    fn account_change_at_publish_discards_sibling_quota_but_preserves_backoff() {
        let make = || UsageSnapshot {
            status: "ok".into(),
            windows: windows(r#"{"rate_limit":{"primary_window":{"used_percent":23}}}"#),
            codex_details: Some(cached_details("account-a")),
            backoff_until: 999_000,
            ..Default::default()
        };
        let same = snapshot_for_account(make(), Some(&account_key("account-a")));
        assert_eq!(same.windows.len(), 1);
        for changed in [Some(account_key("account-b")), None] {
            let snapshot = snapshot_for_account(make(), changed.as_deref());
            assert!(snapshot.windows.is_empty());
            assert!(snapshot.codex_details.is_none());
            assert_eq!(snapshot.status, "none");
            assert_eq!(snapshot.backoff_until, 999_000);
        }
    }

    #[test]
    fn profile_preserves_authoritative_statistics_and_daily_zero() {
        let statistics = parse_statistics(&serde_json::json!({"stats": {
            "lifetime_tokens": 34567, "peak_daily_tokens": 8000,
            "longest_running_turn_sec": 91.25, "current_streak_days": 4, "longest_streak_days": 11,
            "daily_usage_buckets": [{"start_date": "2026-09-28", "tokens": 0}, {"start_date": "2026-09-29", "tokens": 27}]
        }})).ok().unwrap();
        assert_eq!(statistics.lifetime_tokens, Some(34567));
        assert_eq!(statistics.peak_daily_tokens, Some(8000));
        assert_eq!(statistics.longest_running_turn_sec, Some(91.25));
        assert_eq!(statistics.current_streak_days, Some(4));
        assert_eq!(statistics.longest_streak_days, Some(11));
        let days = statistics.daily_usage_buckets.unwrap();
        assert_eq!(
            days[0],
            CodexDailyUsage {
                start_date: "2026-09-28".into(),
                tokens: 0
            }
        );
        assert!(!days.iter().any(|day| day.start_date == "2026-09-30"));
    }

    #[test]
    fn missing_profile_values_are_unknown_and_empty_history_is_observed() {
        for value in [serde_json::json!({}), serde_json::json!({"stats": null})] {
            assert!(parse_statistics(&value).is_err());
        }
        let absent = parse_statistics(
            &serde_json::json!({"stats": {"lifetime_tokens": -1, "longest_running_turn_sec": -2}}),
        )
        .ok()
        .unwrap();
        assert!(absent.lifetime_tokens.is_none());
        assert!(absent.peak_daily_tokens.is_none());
        assert!(absent.longest_running_turn_sec.is_none());
        assert!(absent.daily_usage_buckets.is_none());
        let empty = parse_statistics(
            &serde_json::json!({"stats": {"lifetime_tokens": 0, "daily_usage_buckets": []}}),
        )
        .ok()
        .unwrap();
        assert_eq!(empty.lifetime_tokens, Some(0));
        assert_eq!(empty.daily_usage_buckets, Some(vec![]));
    }

    #[test]
    fn profile_days_validate_calendar_and_last_duplicate_wins() {
        assert!(valid_day("2024-02-29"));
        for invalid in [
            "2026-02-29",
            "2026-09-31",
            "2026-9-01",
            "2026-09-01T00:00:00Z",
        ] {
            assert!(!valid_day(invalid), "{invalid}");
        }
        let statistics = parse_statistics(&serde_json::json!({"stats": {"daily_usage_buckets": [
            {"start_date": "2026-09-29", "tokens": 8}, {"start_date": "2026-09-28", "tokens": 2},
            {"start_date": "2026-09-29", "tokens": 19}
        ]}}))
        .ok()
        .unwrap();
        assert_eq!(
            statistics.daily_usage_buckets.unwrap(),
            vec![
                CodexDailyUsage {
                    start_date: "2026-09-28".into(),
                    tokens: 2
                },
                CodexDailyUsage {
                    start_date: "2026-09-29".into(),
                    tokens: 19
                }
            ]
        );
        for malformed in [
            serde_json::json!({"start_date":"2026-02-30","tokens":1}),
            serde_json::json!({"start_date":"2026-09-29","tokens":-1}),
            serde_json::Value::Null,
        ] {
            let statistics = parse_statistics(&serde_json::json!({"stats": {"peak_daily_tokens": 99, "daily_usage_buckets": [malformed]}})).ok().unwrap();
            assert_eq!(statistics.peak_daily_tokens, Some(99));
            assert!(statistics.daily_usage_buckets.is_none());
        }
    }

    #[test]
    fn credit_count_is_authoritative_and_expiry_is_earliest_future() {
        let now = 1_790_812_800_000; // 2026-10-01 UTC, generated test data only.
        let credits = parse_credits(
            &serde_json::json!({"available_count": 7, "credits": [
                {"status":"available","expires_at":"2026-09-30T00:00:00Z"},
                {"status":"available","expires_at":"2026-10-04T00:00:00Z"},
                {"status":"available","expires_at":"2026-10-02T00:00:00Z"},
                {"status":"redeemed","expires_at":"2026-10-01T01:00:00Z"}, null
            ]}),
            now,
        )
        .ok()
        .unwrap();
        assert_eq!(credits.available_count, 7);
        let expected = chrono::DateTime::parse_from_rfc3339("2026-10-02T00:00:00Z")
            .unwrap()
            .timestamp_millis() as u64;
        assert_eq!(credits.next_expiry_ms, Some(expected));
        assert_eq!(credits.fetched_at, now);
        let reread = parse_credits(&serde_json::json!({"available_count": 7, "credits": [{"status":"available","expires_at":"2026-10-02T00:00:00Z"}]}), expected + 1).ok().unwrap();
        assert_eq!(
            reread.available_count, 7,
            "expiry must not invent a lower backend count"
        );
        assert_eq!(reread.next_expiry_ms, None);
    }

    #[test]
    fn malformed_credit_fallback_is_unknown_not_zero() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"credits": null}),
            serde_json::json!({"credits":[{}]}),
            serde_json::json!({"credits":[null]}),
            serde_json::json!({"credits":[{"status":null}]}),
            serde_json::json!({"credits":[{"status":"new-status"}]}),
            serde_json::json!({"available_count":-1,"credits":[]}),
        ] {
            assert!(parse_credits(&value, 0).is_err(), "{value}");
        }
        let empty = parse_credits(&serde_json::json!({"credits":[]}), 0)
            .ok()
            .unwrap();
        assert_eq!(empty.available_count, 0);
        for value in [
            serde_json::json!({"available_count":4}),
            serde_json::json!({"available_count":4,"credits":[{},null,{"status":null}]}),
        ] {
            assert_eq!(parse_credits(&value, 0).ok().unwrap().available_count, 4);
        }
        let listed = parse_credits(&serde_json::json!({"credits":[{"status":"available"},{"status":"redeemed"},{"status":"expired"}]}), 0).ok().unwrap();
        assert_eq!(listed.available_count, 1);
    }

    #[test]
    fn successful_zero_credit_count_has_no_expiry() {
        let credits = parse_credits(&serde_json::json!({"available_count":0,"credits":[{"status":"available","expires_at":"2026-10-05T00:00:00Z"}]}), 0).ok().unwrap();
        assert_eq!(credits.available_count, 0);
        assert_eq!(credits.next_expiry_ms, None);
    }

    #[test]
    fn auxiliary_failures_keep_matching_cached_readings_and_original_dates() {
        let credential = synthetic_credential("account-a");
        let (details, backoff) = fetch_details_with(
            &credential,
            Some("pro".into()),
            Some(cached_details("account-a")),
            |_| Err(LiveErr::Other("synthetic failure".into())),
            || 300,
        );
        assert_eq!(details.statistics.unwrap().lifetime_tokens, Some(7));
        assert_eq!(details.fetched_at, 100);
        assert_eq!(details.credits.unwrap().fetched_at, 200);
        assert_eq!(
            details.statistics_error.as_deref(),
            Some("Reading unavailable")
        );
        assert_eq!(
            details.credits_error.as_deref(),
            Some("Reading unavailable")
        );
        assert_eq!(backoff, 0);
    }

    #[test]
    fn changed_account_discards_cached_values_on_optional_failure() {
        let (details, _) = fetch_details_with(
            &synthetic_credential("account-b"),
            None,
            Some(cached_details("account-a")),
            |_| Err(LiveErr::NeedsAuth),
            || 300,
        );
        assert_eq!(details.account_key, account_key("account-b"));
        assert!(details.statistics.is_none());
        assert!(details.credits.is_none());
        assert_eq!(
            details.statistics_error.as_deref(),
            Some("Sign-in unavailable")
        );
    }

    #[test]
    fn unavailable_profile_does_not_block_successful_credit_reading() {
        let (details, _) = fetch_details_with(
            &synthetic_credential("account-a"),
            None,
            None,
            |endpoint| match endpoint {
                DetailEndpoint::Profile => Err(LiveErr::NeedsAuth),
                DetailEndpoint::Credits => Ok(serde_json::json!({"available_count":0})),
            },
            || 300,
        );
        assert!(details.statistics.is_none());
        assert_eq!(details.credits.unwrap().available_count, 0);
        assert!(details.credits_error.is_none());
    }

    #[test]
    fn profile_429_stops_remaining_requests_and_starts_hold_at_response() {
        let mut calls = 0;
        let mut clock = [1000, 16_000].into_iter();
        let (details, backoff) = fetch_details_with(
            &synthetic_credential("account-a"),
            None,
            None,
            |endpoint| {
                assert!(matches!(endpoint, DetailEndpoint::Profile));
                calls += 1;
                Err(LiveErr::RateLimited(60))
            },
            || clock.next().unwrap(),
        );
        assert_eq!(calls, 1);
        assert_eq!(backoff, 76_000);
        assert_eq!(details.credits_error.as_deref(), Some("Waiting for retry"));
        assert_eq!(details.statistics_error.as_deref(), Some("Rate limited"));
    }

    #[test]
    fn credit_429_preserves_new_profile_and_uses_later_response_time() {
        let mut clock = [1000, 16_000, 31_000].into_iter();
        let (details, backoff) = fetch_details_with(
            &synthetic_credential("account-a"),
            None,
            None,
            |endpoint| match endpoint {
                DetailEndpoint::Profile => Ok(serde_json::json!({"stats":{"lifetime_tokens":42}})),
                DetailEndpoint::Credits => Err(LiveErr::RateLimited(120)),
            },
            || clock.next().unwrap(),
        );
        assert_eq!(backoff, 151_000);
        assert_eq!(details.statistics.unwrap().lifetime_tokens, Some(42));
        assert_eq!(details.fetched_at, 16_000);
        assert_eq!(details.credits_error.as_deref(), Some("Rate limited"));
    }

    #[test]
    fn retry_after_accepts_seconds_and_http_date_without_overflow() {
        assert_eq!(retry_after_secs(Some(" 120 "), 0), 120);
        assert_eq!(
            retry_after_secs(Some("Thu, 01 Oct 2026 00:00:02 GMT"), 1_790_812_801_001),
            1
        );
        for invalid in [None, Some("bad"), Some("-2")] {
            assert_eq!(retry_after_secs(invalid, 0), 0);
        }
        assert_eq!(
            retry_after_secs(Some("Thu, 01 Oct 2026 00:00:02 GMT"), u64::MAX),
            0
        );
    }

    #[test]
    fn plan_labels_are_bounded_single_line_metadata() {
        assert_eq!(plan_label(Some(" pro ")).as_deref(), Some("pro"));
        for label in ["", "\n", "pro\nprivate"] {
            assert!(plan_label(Some(label)).is_none());
        }
        assert!(plan_label(Some(&"x".repeat(81))).is_none());
    }

    #[test]
    fn app_server_uses_core_bucket_not_legacy_spark() {
        let value = serde_json::json!({"rateLimits":{"limitId":"codex_bengalfox","primary":{"usedPercent":0}},
            "rateLimitsByLimitId":{"codex":{"limitId":"codex","primary":{"usedPercent":32,"windowDurationMins":10080,"resetsAt":1789878630}}}});
        let snapshot = app_server_snapshot(&value).unwrap();
        assert_eq!(snapshot.windows[0].used, 0.32);
        assert_eq!(snapshot.windows[0].label, "Weekly limit");
        assert_eq!(snapshot.windows[0].resets_at, Some(1789878630000));
        assert!(app_server_snapshot(&serde_json::json!({"rateLimits":{"limitId":"codex_bengalfox","primary":{"usedPercent":0}}})).is_none());
    }

    #[test]
    fn app_server_legacy_core_and_both_windows_are_supported() {
        let value = serde_json::json!({"rateLimits": {
            "primary": {"usedPercent": 25, "windowDurationMins": 300, "resetsAt": 1800000000u64},
            "secondary": {"usedPercent": 42, "windowDurationMins": 10080}
        }, "rateLimitsByLimitId": null});
        let snap = app_server_snapshot(&value).unwrap();
        assert_eq!(ids(&snap.windows), ["primary", "secondary"]);
        assert_eq!(labels(&snap.windows), ["5h limit", "Weekly limit"]);
        assert_eq!(snap.windows[0].used, 0.25);
        assert_eq!(snap.windows[1].used, 0.42);
        assert_eq!(snap.windows[0].resets_at, Some(1800000000000));
        assert_eq!(snap.windows[1].resets_at, None);
    }

    #[test]
    fn app_server_missing_core_or_usage_is_not_zero() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"rateLimits": {"primary": {"usedPercent": null}}}),
            serde_json::json!({"rateLimitsByLimitId": {}, "rateLimits": {"primary": {"usedPercent": 7}}}),
            serde_json::json!({"rateLimitsByLimitId": {"codex": null}}),
            serde_json::json!({"rateLimitsByLimitId": {"codex": {"limitId": "other", "primary": {"usedPercent": 7}}}}),
        ] {
            assert!(app_server_snapshot(&value).is_none(), "{value}");
        }
    }

    #[test]
    fn app_server_skips_malformed_windows_and_clamps_percentages() {
        let value = serde_json::json!({"rateLimits": {"limitId": "codex",
            "primary": {"usedPercent": -5}, "secondary": {"usedPercent": 150}}});
        let snap = app_server_snapshot(&value).unwrap();
        assert_eq!(snap.windows[0].used, 0.0);
        assert_eq!(snap.windows[1].used, 1.0);
        let value = serde_json::json!({"rateLimits": {"limitId": "codex",
            "primary": "invalid", "secondary": {"usedPercent": 20}}});
        assert_eq!(ids(&app_server_snapshot(&value).unwrap().windows), ["secondary"]);
    }

    #[test]
    fn rollout_keeps_legacy_core_and_filters_camel_case_buckets() {
        let core = r#"{"rate_limits":{"secondary":{"used_percent":45,"window_minutes":10080}}}"#;
        let other = r#"{"rate_limits":{"limitId":"other","primary":{"used_percent":1}}}"#;
        let (ws, _, _) = snapshot_from_rollout(&format!("{core}\n{other}")).unwrap();
        assert_eq!(ids(&ws), ["secondary"]);
        assert_eq!(ws[0].used, 0.45);
        assert!(snapshot_from_rollout(other).is_none());
    }

    #[test]
    fn rollout_ignores_newer_spark_events() {
        let core = r#"{"timestamp":"2026-09-14T07:00:00Z","payload":{"rate_limits":{"limit_id":"codex","primary":{"used_percent":32,"window_minutes":10080}}}}"#;
        let spark = r#"{"timestamp":"2026-09-14T07:00:01Z","payload":{"rate_limits":{"limit_id":"codex_bengalfox","primary":{"used_percent":0,"window_minutes":300}}}}"#;
        let (ws, _, _) = snapshot_from_rollout(&format!("{core}\n{spark}")).unwrap();
        assert_eq!(ws[0].used, 0.32);
        assert_eq!(ws[0].label, "Weekly limit");
        assert!(snapshot_from_rollout(spark).is_none());
    }

    #[test]
    fn resumed_thread_in_old_date_directory_is_found() {
        let root = std::env::temp_dir().join(format!("matra-rollout-test-{}-{}", std::process::id(), now_ms()));
        std::fs::create_dir(&root).unwrap();
        for day in ["2026/07/31", "2026/09/10", "2026/09/11", "2026/09/12"] {
            std::fs::create_dir_all(root.join("sessions").join(day)).unwrap();
        }
        let earlier = UNIX_EPOCH + Duration::from_secs(1700000000);
        for day in ["2026/09/10", "2026/09/11", "2026/09/12"] {
            let file = std::fs::File::create(root.join("sessions").join(day).join("rollout-inactive.jsonl")).unwrap();
            file.set_times(std::fs::FileTimes::new().set_modified(earlier)).unwrap();
        }
        let active = root.join("sessions/2026/07/31/rollout-active.jsonl");
        std::fs::write(&active, "{}").unwrap();
        // The bounded directory fallback intentionally cannot see this old directory.
        assert_ne!(newest_rollout_in(&root), Some(active.clone()));
        assert!(!root.join("state_5.sqlite").exists(), "read-only lookup must not create a database");
        let db = rusqlite::Connection::open(root.join("state_5.sqlite")).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE threads (rollout_path TEXT, archived INTEGER, updated_at_ms INTEGER);
            CREATE INDEX recent_threads ON threads(archived, updated_at_ms DESC);").unwrap();
        db.execute("INSERT INTO threads VALUES (?1, 0, 100)", [active.to_str().unwrap()]).unwrap();
        // Keep the writer open: the read-only connection must see the committed WAL update.
        assert_eq!(newest_rollout_in(&root), Some(active.clone()));
        db.execute("UPDATE threads SET archived = 1", []).unwrap();
        assert_ne!(newest_rollout_in(&root), Some(active));
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rollout_index_skips_missing_paths_and_accepts_legacy_timestamp_column() {
        let root = std::env::temp_dir().join(format!("matra-index-test-{}-{}", std::process::id(), now_ms()));
        std::fs::create_dir(&root).unwrap();
        let database = root.join("state_5.sqlite");
        let active = root.join("rollout-active.jsonl");
        std::fs::write(&active, "{}").unwrap();
        let db = rusqlite::Connection::open(&database).unwrap();
        db.execute_batch("CREATE TABLE threads (rollout_path TEXT, archived INTEGER, updated_at INTEGER);").unwrap();
        db.execute("INSERT INTO threads VALUES (?1, 0, 1)", [active.to_str().unwrap()]).unwrap();
        db.execute("INSERT INTO threads VALUES (?1, 0, 2)", [root.join("rollout-missing.jsonl").to_str().unwrap()]).unwrap();
        assert_eq!(indexed_rollout(&database), Some(active));
        // Bound file metadata work even when the newest entries are unavailable.
        for stamp in 3..10 {
            db.execute("INSERT INTO threads VALUES ('rollout-missing.jsonl', 0, ?1)", [stamp]).unwrap();
        }
        assert_eq!(indexed_rollout(&database), None);
        drop(db);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn corrupt_rollout_index_uses_bounded_directory_fallback() {
        let root = std::env::temp_dir().join(format!("matra-index-fallback-{}-{}", std::process::id(), now_ms()));
        let day = root.join("sessions/2026/09/16");
        std::fs::create_dir_all(&day).unwrap();
        let active = day.join("rollout-active.jsonl");
        std::fs::write(&active, "{}").unwrap();
        std::fs::write(root.join("state_5.sqlite"), "not a SQLite database").unwrap();
        assert_eq!(newest_rollout_in(&root), Some(active));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "Reads quota through the installed signed-in native Codex client; opt in explicitly"]
    fn live_native_quota() {
        let snap = read_app_server().expect("native quota read should succeed for this signed-in client");
        assert_eq!(snap.status, "ok");
        assert!(!snap.windows.is_empty());
        assert!(snap.windows.iter().all(|window| matches!(window.id.as_str(), "primary" | "secondary")));
    }

    fn windows(json: &str) -> Vec<LimitWindow> {
        windows_from_usage(&serde_json::from_str(json).unwrap())
    }

    fn ids(ws: &[LimitWindow]) -> Vec<&str> {
        ws.iter().map(|w| w.id.as_str()).collect()
    }

    fn labels(ws: &[LimitWindow]) -> Vec<&str> {
        ws.iter().map(|w| w.label.as_str()).collect()
    }

    fn groups(ws: &[LimitWindow]) -> Vec<Option<&str>> {
        ws.iter().map(|w| w.group.as_deref()).collect()
    }

    #[test]
    fn extra_spark_and_code_review_follow_primary_secondary() {
        let ws = windows(
            r#"{
            "rate_limit":{
              "primary_window":{"used_percent":25,"limit_window_seconds":18000,"reset_at":1800001000},
              "secondary_window":{"used_percent":10,"limit_window_seconds":604800,"reset_at":1800600000}},
            "additional_rate_limits":[{"limit_name":"Spark","rate_limit":{
              "primary_window":{"used_percent":99,"limit_window_seconds":18000}}}],
            "code_review_rate_limit":{"primary_window":{"used_percent":90,"limit_window_seconds":604800}},
            "credits":{"balance":"100"},"model_usage":{"spark":99}
        }"#,
        );
        assert_eq!(ids(&ws), ["primary", "secondary", "spark", "code-review"]);
        assert_eq!(labels(&ws), ["5h limit", "Weekly limit", "5h limit", "Weekly limit"]);
        assert_eq!(groups(&ws), [None, None, Some("Spark"), Some("Code review")]);
        assert!((ws[0].used - 0.25).abs() < 1e-9);
        assert!((ws[2].used - 0.99).abs() < 1e-9);
        assert!((ws[3].used - 0.90).abs() < 1e-9);
        assert_eq!(ws[0].resets_at, Some(1_800_001_000_000));
    }

    #[test]
    fn spark_matches_limit_name_or_metered_feature_case_insensitively() {
        // "GPT-5.3-Codex-Spark" still contains the substring "Spark", so it
        // would pass a case-sensitive contains("Spark"). SPARK / spark would not.
        for (field, name) in [
            ("limit_name", "SPARK"),
            ("limit_name", "spark"),
            ("metered_feature", "GPT-5.3-Codex-SPARK"),
            ("metered_feature", "gpt-5.3-codex-spark"),
        ] {
            let ws = windows(&format!(
                r#"{{
                "rate_limit":{{"primary_window":{{"used_percent":1,"limit_window_seconds":18000}}}},
                "additional_rate_limits":[{{"{field}":"{name}","rate_limit":{{
                  "primary_window":{{"used_percent":40,"limit_window_seconds":18000}},
                  "secondary_window":{{"used_percent":5,"limit_window_seconds":604800}}}}}}]
            }}"#
            ));
            assert_eq!(ids(&ws), ["primary", "spark", "spark-secondary"], "{field}={name}");
            assert_eq!(groups(&ws)[1..], [Some("Spark"), Some("Spark")], "{field}={name}");
            assert_eq!(labels(&ws)[1..], ["5h limit", "Weekly limit"], "{field}={name}");
        }
    }

    #[test]
    fn extras_alone_are_still_a_reading() {
        let ws = windows(
            r#"{"additional_rate_limits":[{"limit_name":"Spark","rate_limit":{
              "primary_window":{"used_percent":40,"limit_window_seconds":18000},
              "secondary_window":{"used_percent":70,"limit_window_seconds":604800}}}]}"#,
        );
        assert_eq!(ids(&ws), ["spark", "spark-secondary"]);
        assert_eq!(groups(&ws), [Some("Spark"), Some("Spark")]);
        assert_eq!(labels(&ws), ["5h limit", "Weekly limit"]);
        assert!((ws[0].used - 0.40).abs() < 1e-9);
        assert!((ws[1].used - 0.70).abs() < 1e-9);
    }

    #[test]
    fn non_spark_additional_limits_are_ignored() {
        let ws = windows(
            r#"{
            "rate_limit":{"primary_window":{"used_percent":1,"limit_window_seconds":18000}},
            "additional_rate_limits":[{"limit_name":"codex_other","metered_feature":"codex_other","rate_limit":{
              "primary_window":{"used_percent":70,"limit_window_seconds":3600}}}]
        }"#,
        );
        assert_eq!(ids(&ws), ["primary"]);
    }

    #[test]
    fn empty_additional_rate_limits_leave_the_main_windows() {
        let ws = windows(
            r#"{
            "rate_limit":{
              "primary_window":{"used_percent":25,"limit_window_seconds":18000},
              "secondary_window":{"used_percent":10,"limit_window_seconds":604800}},
            "additional_rate_limits":[]
        }"#,
        );
        assert_eq!(ids(&ws), ["primary", "secondary"]);
        assert_eq!(groups(&ws), [None, None]);
    }

    #[test]
    fn extras_without_used_percent_are_skipped() {
        let ws = windows(
            r#"{
            "rate_limit":{"primary_window":{"used_percent":1,"limit_window_seconds":18000}},
            "additional_rate_limits":[{"limit_name":"Spark","rate_limit":{
              "primary_window":{"used_percent":null,"limit_window_seconds":18000},
              "secondary_window":{"used_percent":12,"limit_window_seconds":604800}}}],
            "code_review_rate_limit":{
              "primary_window":{"limit_window_seconds":604800},
              "secondary_window":{"used_percent":8,"limit_window_seconds":18000}}
        }"#,
        );
        assert_eq!(ids(&ws), ["primary", "spark-secondary", "code-review-secondary"]);
        assert_eq!(groups(&ws)[1..], [Some("Spark"), Some("Code review")]);
        assert_eq!(ws[1].label, "Weekly limit");
        assert_eq!(ws[2].label, "5h limit");
    }

    #[test]
    fn malformed_extras_do_not_drop_the_main_windows() {
        let ws = windows(
            r#"{
            "rate_limit":{"primary_window":{"used_percent":25,"limit_window_seconds":18000}},
            "additional_rate_limits":[
              "nope",
              42,
              null,
              {"limit_name":"Spark"},
              {"limit_name":"Spark","rate_limit":"nope"},
              {"limit_name":"Spark","rate_limit":{"primary_window":{
                "used_percent":40,"limit_window_seconds":18000,"reset_after_seconds":1e20}}},
              {"limit_name":"Spark","rate_limit":{"primary_window":{
                "used_percent":15,"limit_window_seconds":18000}}}
            ],
            "code_review_rate_limit":"nope"
        }"#,
        );
        assert_eq!(ids(&ws), ["primary", "spark"]);
        assert_eq!(groups(&ws), [None, Some("Spark")]);
        assert!((ws[1].used - 0.40).abs() < 1e-9);
        assert!(ws[1].resets_at.is_some());
    }

    #[test]
    fn two_spark_extras_do_not_duplicate_window_ids() {
        let ws = windows(
            r#"{
            "rate_limit":{
              "primary_window":{"used_percent":25,"limit_window_seconds":18000},
              "secondary_window":{"used_percent":10,"limit_window_seconds":604800}},
            "additional_rate_limits":[
              {"limit_name":"Spark","rate_limit":{
                "primary_window":{"used_percent":40,"limit_window_seconds":18000}}},
              {"limit_name":"GPT-5.3-Codex-Spark","metered_feature":"spark","rate_limit":{
                "primary_window":{"used_percent":99,"limit_window_seconds":18000},
                "secondary_window":{"used_percent":12,"limit_window_seconds":604800}}}
            ]
        }"#,
        );
        assert_eq!(ids(&ws), ["primary", "secondary", "spark", "spark-secondary"]);
        assert_eq!(groups(&ws), [None, None, Some("Spark"), Some("Spark")]);
        assert_eq!(labels(&ws), ["5h limit", "Weekly limit", "5h limit", "Weekly limit"]);
        assert!((ws[2].used - 0.40).abs() < 1e-9);
        assert!((ws[3].used - 0.12).abs() < 1e-9);
    }

    #[test]
    fn a_non_array_additional_rate_limits_is_ignored() {
        for extras in [
            r#"{"x":{"limit_name":"Spark","rate_limit":{"primary_window":{"used_percent":9,"limit_window_seconds":18000}}}}"#,
            r#""nope""#,
            "null",
        ] {
            let ws = windows(&format!(
                r#"{{"rate_limit":{{"primary_window":{{"used_percent":1,"limit_window_seconds":18000}}}},"additional_rate_limits":{extras}}}"#
            ));
            assert_eq!(ids(&ws), ["primary"], "{extras}");
        }
    }

    #[test]
    fn a_monthly_primary_window_is_not_dropped() {
        let ws = windows(
            r#"{"rate_limit":{"primary_window":{"used_percent":16,"limit_window_seconds":2592000,
            "reset_after_seconds":1838382,"reset_at":1790585722},"secondary_window":null},
             "plan_type":"free"}"#,
        );
        assert_eq!(ids(&ws), ["primary"]);
        assert_eq!(ws[0].label, "Monthly limit");
        assert!((ws[0].used - 0.16).abs() < 1e-4);
    }
}
