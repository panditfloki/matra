//! Windows ports of CodeNotch's Copilot/OpenCode/CommandCode/Kimi readers.
//! Upstream MIT attribution is retained in native/LICENSE. Credentials are borrowed
//! read-only, never emitted, persisted or logged. Each reader is explicitly opt-in.
use crate::usage::{LimitWindow, UsageSnapshot};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

pub const IDS: [&str; 4] = ["copilot", "opencode", "commandcode", "kimi"];
#[derive(Default)]
struct Entry {
    snapshot: UsageSnapshot,
    next: u64,
    generation: u64,
    last_attempt: u64,
}
static STORE: OnceLock<Mutex<BTreeMap<String, Entry>>> = OnceLock::new();
fn store() -> &'static Mutex<BTreeMap<String, Entry>> {
    STORE.get_or_init(Default::default)
}
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn absent() -> UsageSnapshot {
    UsageSnapshot {
        status: "absent".into(),
        ..Default::default()
    }
}
pub fn snapshot(id: &str) -> UsageSnapshot {
    store()
        .lock()
        .unwrap()
        .get(id)
        .map(|s| s.snapshot.clone())
        .unwrap_or_else(absent)
}
pub fn request_refresh(id: &str) {
    if let Some(s) = store().lock().unwrap().get_mut(id) {
        s.next = s
            .snapshot
            .backoff_until
            .max(s.last_attempt.saturating_add(30_000));
    }
}
#[tauri::command]
pub fn get_extra_usage() -> BTreeMap<String, UsageSnapshot> {
    IDS.iter().map(|id| ((*id).into(), snapshot(id))).collect()
}
#[tauri::command]
pub fn set_extra_provider(app: AppHandle, id: String, on: bool) -> Result<(), String> {
    if !IDS.contains(&id.as_str()) {
        return Err("Unknown provider.".into());
    }
    {
        let st = app.state::<crate::AppState>();
        let mut cfg = st.cfg.lock().unwrap();
        let mut next = cfg.clone();
        next.extra_providers.retain(|p| p != &id);
        if on {
            next.extra_providers.push(id.clone());
            if !next.notch_slots.is_empty() && !next.notch_slots.iter().any(|s| s.provider == id) {
                next.notch_slots.push(crate::config::TraySlot {
                    provider: id.clone(),
                });
            }
        }
        crate::config::save_checked(&next)
            .map_err(|_| "Could not save provider choice. Nothing changed.".to_string())?;
        *cfg = next;
    }
    {
        let mut data = store().lock().unwrap();
        let s = data.entry(id).or_default();
        s.generation += 1;
        s.next = 0;
        s.snapshot = if on {
            UsageSnapshot {
                status: "needsAuth".into(),
                note: "Checking the tool's existing sign-in...".into(),
                ..Default::default()
            }
        } else {
            absent()
        };
    }
    let _ = app.emit("extra_usage", get_extra_usage());
    let slots = app
        .state::<crate::AppState>()
        .cfg
        .lock()
        .unwrap()
        .notch_slots
        .clone();
    let _ = app.emit("notch_slots", slots);
    Ok(())
}
fn clean(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 8192 && !s.contains(['\r', '\n']))
        .map(str::to_owned)
}
fn json_file(path: &Path) -> Option<Value> {
    let mut b = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut b)
        .ok()?;
    if b.len() > 1024 * 1024 {
        return None;
    }
    serde_json::from_slice(&b).ok()
}
fn key(id: &str, root: &Value, clock: u64) -> Option<String> {
    match id {
        "opencode" => {
            let entry = &root["opencode-go"];
            clean(entry.as_str()).or_else(|| {
                ["key", "apiKey", "api_key", "token", "accessToken"]
                    .iter()
                    .find_map(|k| clean(entry[*k].as_str()))
            })
        }
        "commandcode" => clean(root["apiKey"].as_str()),
        "kimi"
            if root["expires_at"]
                .as_f64()
                .is_some_and(|v| v.is_finite() && v * 1000.0 > clock as f64) =>
        {
            clean(root["access_token"].as_str())
        }
        _ => None,
    }
}
fn gh_token() -> Option<String> {
    // Explicit hostname prevents an enterprise credential from reaching github.com.
    let executable = std::env::split_paths(&std::env::var_os("PATH")?)
        .filter(|p| p.is_absolute())
        .map(|p| p.join("gh.exe"))
        .find(|p| p.is_file())?;
    let mut cmd = std::process::Command::new(executable);
    cmd.args(["auth", "token", "--hostname", "github.com"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let mut child = cmd.spawn().ok()?;
    let stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.take(8193).read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    let success = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let bytes = reader.join().ok()?;
    if !success || bytes.len() > 8192 {
        return None;
    }
    clean(std::str::from_utf8(&bytes).ok())
}
fn credential(id: &str) -> Option<String> {
    if id == "copilot" {
        return ["GH_TOKEN", "GITHUB_TOKEN"]
            .iter()
            .find_map(|k| clean(std::env::var(k).ok().as_deref()))
            .or_else(gh_token);
    }
    if id == "commandcode" {
        if let Some(v) = clean(std::env::var("COMMAND_CODE_API_KEY").ok().as_deref()) {
            return Some(v);
        }
    }
    let home = dirs::home_dir()?;
    let paths = match id {
        "opencode" => {
            let mut paths = vec![];
            if let Some(p) = std::env::var_os("XDG_DATA_HOME").filter(|v| !v.is_empty()) {
                paths.push(PathBuf::from(p).join("opencode/auth.json"));
            }
            paths.push(home.join(".local/share/opencode/auth.json"));
            if let Some(p) = dirs::config_dir() {
                paths.push(p.join("opencode/auth.json"));
            }
            paths
        }
        "commandcode" => vec![home.join(".commandcode/auth.json")],
        "kimi" => vec![std::env::var_os("KIMI_CODE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".kimi-code"))
            .join("credentials/kimi-code.json")],
        _ => vec![],
    };
    paths
        .iter()
        .find_map(|p| json_file(p).and_then(|v| key(id, &v, now())))
}
fn number(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str()?.parse().ok())
        .filter(|n| n.is_finite() && *n >= 0.0)
}
fn date(v: &Value) -> Option<u64> {
    if let Some(n) = number(v).filter(|n| *n > 0.0) {
        return Some(if n > 10_000_000_000.0 {
            n as u64
        } else {
            (n * 1000.0) as u64
        });
    }
    chrono::DateTime::parse_from_rfc3339(v.as_str()?)
        .ok()
        .and_then(|d| u64::try_from(d.timestamp_millis()).ok())
}
fn fraction(id: &str, label: &str, used: f64, reset: &Value) -> LimitWindow {
    LimitWindow {
        id: id.into(),
        label: label.into(),
        used: used.clamp(0.0, 1.0),
        resets_at: date(reset),
        ..Default::default()
    }
}
fn ratio(
    id: &str,
    label: &str,
    row: &Value,
    used: &str,
    limit: &str,
    reset: &str,
) -> Option<LimitWindow> {
    let u = number(&row[used])?;
    let cap = number(&row[limit])?;
    (cap > 0.0).then(|| fraction(id, label, u / cap, &row[reset]))
}
fn parse(id: &str, root: &Value) -> Vec<LimitWindow> {
    match id {
        "opencode" => [
            ("rolling", "5h limit"),
            ("weekly", "Weekly limit"),
            ("monthly", "Monthly limit"),
        ]
        .into_iter()
        .filter_map(|(id, label)| {
            let row = &root["usage"][id];
            Some(fraction(
                id,
                label,
                number(&row["percent"])? / 100.0,
                &row["resetsAt"],
            ))
        })
        .collect(),
        "copilot" => [
            ("premium_interactions", "Premium requests"),
            ("chat", "Chat requests"),
            ("completions", "Completions"),
        ]
        .into_iter()
        .filter_map(|(id, label)| {
            let q = &root["quota_snapshots"][id];
            if q["unlimited"].as_bool() == Some(true) {
                return None;
            }
            let cap = number(&q["entitlement"])?;
            if cap <= 0.0 {
                return None;
            }
            let used =
                number(&q["used"]).or_else(|| Some((cap - number(&q["remaining"])?).max(0.0)))?;
            let reset = ["reset_date", "reset_at", "resets_at"]
                .iter()
                .find_map(|k| date(&q[*k]))
                .or_else(|| date(&root["quota_reset_date"]));
            let mut w = fraction(id, label, used / cap, &Value::Null);
            w.resets_at = reset;
            Some(w)
        })
        .collect(),
        "kimi" => {
            let mut out = Vec::new();
            if let Some(rows) = root["limits"].as_array() {
                for row in rows {
                    let win = &row["window"];
                    let duration = number(&win["duration"]);
                    let unit = win["timeUnit"].as_str();
                    let kind = match (unit, duration) {
                        (Some("TIME_UNIT_MINUTE"), Some(300.0))
                        | (Some("TIME_UNIT_HOUR"), Some(5.0)) => Some(("rolling", "5h limit")),
                        (Some("TIME_UNIT_WEEK"), Some(1.0)) => Some(("weekly", "Weekly limit")),
                        _ => None,
                    };
                    if let Some((id, label)) = kind {
                        if !out.iter().any(|w: &LimitWindow| w.id == id) {
                            if let Some(w) =
                                ratio(id, label, &row["detail"], "used", "limit", "resetTime")
                            {
                                out.push(w)
                            }
                        }
                    }
                }
            }
            if !out.iter().any(|w| w.id == "weekly") {
                if let Some(w) = ratio(
                    "weekly",
                    "Weekly limit",
                    &root["usage"],
                    "used",
                    "limit",
                    "resetTime",
                ) {
                    out.push(w)
                }
            }
            out
        }
        _ => vec![],
    }
}
fn command_windows(summary: &Value, credits: &Value, sub: &Value) -> Vec<LimitWindow> {
    let mut out = vec![];
    if let (Some(used), Some(left)) = (
        number(&summary["totalCost"]),
        number(&credits["credits"]["monthlyCredits"]),
    ) {
        if used + left > 0.0 {
            out.push(fraction(
                "monthly",
                "Monthly limit",
                used / (used + left),
                &sub["currentPeriodEnd"],
            ))
        }
    }
    for (id, label) in [("fiveHour", "5h limit"), ("weekly", "Weekly limit")] {
        if let Some(w) = ratio(
            id,
            label,
            &credits["windowLimits"][id],
            "used",
            "cap",
            "resetAt",
        ) {
            out.push(w)
        }
    }
    out
}
struct Failure {
    status: &'static str,
    note: &'static str,
    retry: u64,
}
fn failure(status: &'static str, note: &'static str) -> Failure {
    Failure {
        status,
        note,
        retry: 300,
    }
}
fn read_json(url: &str, token: &str, command: bool) -> Result<Value, Failure> {
    // Zero redirects: a credential must never follow a redirect to another host.
    let agent = crate::http::builder().redirects(0).build();
    let mut req = agent
        .get(url)
        .set("Authorization", &format!("Bearer {token}"))
        .set("Accept", "application/json")
        .set("User-Agent", "Matra");
    if command {
        req = req
            .set("User-Agent", "command-code-desktop")
            .set("x-command-code-version", "desktop");
    }
    if url.starts_with("https://api.github.com/") {
        req = req.set("X-GitHub-Api-Version", "2022-11-28");
    }
    let response=match req.call(){Ok(r) if r.status()==200=>r,Err(ureq::Error::Status(401|403,_))=>return Err(failure("needsAuth","Sign-in expired or this account cannot access usage. Sign in using the provider's own tool.")),Err(ureq::Error::Status(429,r))=>return Err(Failure{status:"backoff",note:"Provider rate limited this reader. Waiting before retrying.",retry:r.header("Retry-After").and_then(|s|s.parse::<u64>().ok()).unwrap_or(300).max(300)}),Err(ureq::Error::Status(404,_))=>return Err(failure("error","Usage endpoint unavailable for this account or plan.")),_=>return Err(failure("error","Could not read provider usage. Check connection and retry."))};
    let mut bytes = vec![];
    response
        .into_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failure("error", "Could not read usage response."))?;
    if bytes.len() > 1024 * 1024 {
        return Err(failure("error", "Usage response exceeded the size limit."));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| failure("error", "Provider returned invalid usage data."))
}
fn query(base: &str, org: Option<&str>, since: Option<&str>) -> String {
    let mut url = url::Url::parse(base).expect("fixed provider URL");
    {
        let mut q = url.query_pairs_mut();
        if let Some(s) = org {
            q.append_pair("orgId", s);
        }
        if let Some(s) = since {
            q.append_pair("since", s);
        }
    }
    url.into()
}
fn fetch(id: &str) -> Result<Vec<LimitWindow>, Failure> {
    let token = credential(id).ok_or_else(|| {
        failure(
            "needsAuth",
            "No valid sign-in found. Follow this provider's connection instructions in Settings.",
        )
    })?;
    if id == "commandcode" {
        let who = read_json("https://api.commandcode.ai/alpha/whoami", &token, true)?;
        let org = who["org"]["id"].as_str();
        let credits = read_json(
            &query(
                "https://api.commandcode.ai/alpha/billing/credits",
                org,
                None,
            ),
            &token,
            true,
        )?;
        let subroot = read_json(
            &query(
                "https://api.commandcode.ai/alpha/billing/subscriptions",
                org,
                None,
            ),
            &token,
            true,
        )?;
        let sub = subroot.get("data").unwrap_or(&subroot);
        let since = date(&sub["currentPeriodStart"])
            .and_then(|ms| chrono::DateTime::from_timestamp_millis(ms as i64))
            .map(|dt| dt.to_rfc3339());
        // Never mix lifetime spend with the current month's remaining balance.
        let since = since.ok_or_else(|| {
            failure(
                "error",
                "Subscription period missing; monthly usage cannot be calculated safely.",
            )
        })?;
        let summary = read_json(
            &query(
                "https://api.commandcode.ai/alpha/usage/summary",
                org,
                Some(&since),
            ),
            &token,
            true,
        )?;
        return Ok(command_windows(&summary, &credits, sub));
    }
    let endpoint = match id {
        "copilot" => "https://api.github.com/copilot_internal/user",
        "opencode" => "https://opencode.ai/zen/go/v1/usage",
        "kimi" => "https://api.kimi.com/coding/v1/usages",
        _ => return Err(failure("error", "Unknown provider.")),
    };
    Ok(parse(id, &read_json(endpoint, &token, false)?))
}
pub fn start(app: AppHandle) {
    let enabled = app
        .state::<crate::AppState>()
        .cfg
        .lock()
        .unwrap()
        .extra_providers
        .clone();
    {
        let mut data = store().lock().unwrap();
        for id in IDS {
            data.entry(id.into()).or_default().snapshot = if enabled.iter().any(|p| p == id) {
                UsageSnapshot {
                    status: "needsAuth".into(),
                    note: "Checking the tool's existing sign-in...".into(),
                    ..Default::default()
                }
            } else {
                absent()
            };
        }
    }
    for id in IDS {
        let app = app.clone();
        std::thread::spawn(move || loop {
            let enabled = app
                .state::<crate::AppState>()
                .cfg
                .lock()
                .unwrap()
                .extra_providers
                .iter()
                .any(|p| p == id);
            if enabled {
                let generation = {
                    let mut data = store().lock().unwrap();
                    let s = data.entry(id.into()).or_default();
                    if now() >= s.next {
                        s.last_attempt = now();
                        Some(s.generation)
                    } else {
                        None
                    }
                };
                if let Some(generation) = generation {
                    let result = fetch(id);
                    let still_enabled = app
                        .state::<crate::AppState>()
                        .cfg
                        .lock()
                        .unwrap()
                        .extra_providers
                        .iter()
                        .any(|p| p == id);
                    {
                        let mut data = store().lock().unwrap();
                        let s = data.entry(id.into()).or_default();
                        if still_enabled && generation == s.generation {
                            let (snapshot, retry) = match result {
                                Ok(windows) => {
                                    let empty = windows.is_empty();
                                    (UsageSnapshot{status:if empty{"error"}else{"ok"}.into(),windows,fetched_at:now(),note:if empty{"No metered quota with a published denominator was returned."}else{""}.into(),..Default::default()},300)
                                }
                                Err(e) => (
                                    UsageSnapshot {
                                        status: e.status.into(),
                                        note: e.note.into(),
                                        backoff_until: if e.status == "backoff" {
                                            now().saturating_add(e.retry.saturating_mul(1000))
                                        } else {
                                            0
                                        },
                                        ..Default::default()
                                    },
                                    e.retry,
                                ),
                            };
                            // Do not carry an old account's readings through sign-out or failure.
                            s.snapshot = snapshot;
                            s.next = now().saturating_add(retry.saturating_mul(1000));
                        }
                    }
                    let _ = app.emit("extra_usage", get_extra_usage());
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn new_readers_are_off_for_fresh_and_older_configs() {
        let cfg = crate::config::Config::default();
        assert!(cfg.extra_providers.is_empty());
        let mut old = serde_json::to_value(cfg).unwrap();
        old.as_object_mut().unwrap().remove("extra_providers");
        assert!(serde_json::from_value::<crate::config::Config>(old)
            .unwrap()
            .extra_providers
            .is_empty());
    }
    #[test]
    fn transport_never_follows_redirects_and_honours_backoff() {
        use std::io::Write;
        fn reply(response: &'static str) -> Result<Value, Failure> {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}/usage", listener.local_addr().unwrap());
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = [0; 4096];
                let _ = stream.read(&mut bytes);
                stream.write_all(response.as_bytes()).unwrap();
            });
            let result = read_json(&endpoint, "synthetic-test-token", false);
            server.join().unwrap();
            result
        }
        let redirect=reply("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/must-not-follow\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        assert!(redirect.is_err());
        let limited=reply("HTTP/1.1 429 Too Many Requests\r\nRetry-After: 900\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").err().unwrap();
        assert_eq!(limited.status, "backoff");
        assert_eq!(limited.retry, 900);
        assert_eq!(
            reply("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .err()
                .unwrap()
                .status,
            "needsAuth"
        );
    }
    #[test]
    fn credentials_are_provider_scoped() {
        assert!(key("opencode", &json!({"openai":{"key":"secret"}}), 0).is_none());
        assert_eq!(
            key("opencode", &json!({"opencode-go":{"key":"go-key"}}), 0).as_deref(),
            Some("go-key")
        );
        assert!(key(
            "kimi",
            &json!({"access_token":"expired","expires_at":1}),
            2000
        )
        .is_none());
        assert!(key("kimi", &json!({"access_token":"no-expiry"}), 0).is_none());
        assert!(clean(Some("token\nheader")).is_none());
    }
    #[test]
    fn open_code_percent_is_used_and_missing_is_unknown() {
        let w = parse(
            "opencode",
            &json!({"usage":{"rolling":{"percent":17},"weekly":{"percent":90}}}),
        );
        assert_eq!(w[0].used, 0.17);
        assert_eq!(w[0].id, "rolling");
        assert!(parse("opencode", &json!({"usage":{"rolling":{}}})).is_empty());
    }
    #[test]
    fn copilot_skips_unlimited_and_missing_usage() {
        let w = parse(
            "copilot",
            &json!({"quota_snapshots":{"premium_interactions":{"entitlement":100,"remaining":70},"chat":{"unlimited":true,"entitlement":100,"used":10},"completions":{"entitlement":100}}}),
        );
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].used, 0.3);
    }
    #[test]
    fn kimi_string_counts_and_known_windows_only() {
        let w = parse(
            "kimi",
            &json!({"usage":{"used":"12","limit":"100"},"limits":[{"window":{"duration":300,"timeUnit":"TIME_UNIT_MINUTE"},"detail":{"used":"8","limit":"100"}}]}),
        );
        assert_eq!(w[0].id, "rolling");
        assert_eq!(w[0].used, 0.08);
        assert_eq!(w[1].id, "weekly");
        assert!(parse("kimi", &json!({"usage":{"used":"12"}})).is_empty());
    }
    #[test]
    fn command_code_no_invented_zero_or_denominator() {
        let c = json!({"credits":{"monthlyCredits":80}});
        assert_eq!(
            command_windows(&json!({"totalCost":20}), &c, &Value::Null)[0].used,
            0.2
        );
        assert!(command_windows(&json!({}), &c, &Value::Null).is_empty());
        assert!(command_windows(
            &json!({"totalCost":0}),
            &json!({"credits":{"monthlyCredits":0}}),
            &Value::Null
        )
        .is_empty());
        assert_eq!(date(&json!(0)), None);
        let url = query(
            "https://api.commandcode.ai/alpha/usage/summary",
            Some("a&bad=1"),
            None,
        );
        assert!(url.contains("orgId=a%26bad%3D1"));
    }
}
