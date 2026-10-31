//! Higgsfield credits, from the official CLI when it is installed and otherwise from the developer
//! gateway using a credential that already exists. Neither `hf` nor the gateway is asked for anything
//! but a workspace listing.
use super::model::{count_window, fraction_window, number, Failure, Reading};
use super::net;
use serde_json::{json, Value};
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const GATEWAY: &str = "https://fnf-api-gw.higgsfield.ai/fnf/developer/v2alpha/account/workspaces";
/// Higgsfield's own names only. `HF_TOKEN` is Hugging Face's and must never be sent here.
pub const ENV_KEYS: [&str; 2] = ["HIGGSFIELD_API_KEY", "HIGGSFIELD_TOKEN"];
const CLI_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_OUTPUT: u64 = 1024 * 1024;

fn allowance_for(plan: &str) -> Option<f64> {
    match plan.trim().to_lowercase().as_str() {
        "ultra" => Some(3000.0),
        "plus" | "pro" => Some(1200.0),
        "starter" => Some(250.0),
        "basic" => Some(150.0),
        "free" => Some(0.0),
        _ => None,
    }
}

fn nothing(note: &str) -> Failure {
    Failure::error(note)
}

fn capitalise(plan: &str) -> String {
    plan.replace('_', " ")
        .split(' ')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse(root: &Value, source: &str) -> Result<Reading, Failure> {
    let none_found = "No Higgsfield workspaces or credits found.";
    let workspaces: Vec<&Value> = if let Some(list) = root.as_array() {
        list.iter().filter(|v| v.is_object()).collect()
    } else if let Some(list) = root["workspaces"].as_array() {
        list.iter().filter(|v| v.is_object()).collect()
    } else if root.is_object()
        && (root.get("credits").is_some()
            || root.get("plan_type").is_some()
            || root.get("subscription_plan_type").is_some())
    {
        vec![root]
    } else if root.is_object() {
        vec![]
    } else {
        return Err(Failure::transient("Higgsfield returned invalid usage data."));
    };
    // The selected workspace, or the only one. Several with none selected would mean showing some other
    // workspace's balance as this one's, so that is not a reading.
    let active = workspaces
        .iter()
        .find(|w| w["is_selected"].as_bool() == Some(true))
        .or_else(|| (workspaces.len() == 1).then(|| &workspaces[0]))
        .ok_or_else(|| nothing(none_found))?;
    let credits =
        number(&active["credits"]).ok_or_else(|| nothing("No Higgsfield credit balance reported."))?;
    let plan = ["plan_type", "subscription_plan_type", "plan"]
        .iter()
        .find_map(|k| active[*k].as_str());
    let explicit = ["credit_limit", "allowance", "monthly_credits"]
        .iter()
        .find_map(|k| number(&active[*k]));
    let allowance = explicit.or_else(|| plan.and_then(allowance_for)).filter(|a| *a > 0.0);
    let resets_at = ["resets_at", "reset_at", "period_end", "reset_date"]
        .iter()
        .find_map(|k| super::model::rfc3339_ms(&active[*k]));
    // An overdrawn balance has nothing left to spend, not a negative amount.
    let left = credits.max(0.0).round() as i64;
    let mut windows = Vec::new();
    match allowance {
        Some(allowance) => {
            let used = (allowance - credits).max(0.0);
            let mut window = fraction_window("credits", "Credits", used / allowance, resets_at);
            window.derived = explicit.is_none();
            windows.push(window);
            let extra = (credits - allowance).max(0.0).round() as i64;
            if extra > 0 {
                let mut row = count_window("extra_credits", "Extra credits", extra);
                row.resets_at = resets_at;
                windows.push(row);
            }
        }
        // No known allowance, or pay-as-you-go: the balance, without guessing a fraction.
        None => {
            let mut row = count_window("credits", "Credits", left);
            row.resets_at = resets_at;
            windows.push(row);
        }
    }
    Ok(Reading {
        windows,
        plan: plan.map(capitalise),
        details: Some(json!({
            "workspace": active["name"].as_str(),
            "source": source,
            "credits_left": left,
            "allowance": allowance,
        })),
        ..Default::default()
    })
}

/// `hf` is also Hugging Face's CLI, so that name only counts once it resolves into Higgsfield's own
/// package. On Windows an npm install leaves a `hf.cmd` shim whose path says nothing but whose text
/// names the package.
pub fn is_higgsfield(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_lowercase();
    if stem == "higgsfield" {
        return true;
    }
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if resolved.to_string_lossy().to_lowercase().contains("higgsfield") {
        return true;
    }
    let extension = path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_lowercase();
    if matches!(extension.as_str(), "cmd" | "bat") {
        let mut text = Vec::new();
        if std::fs::File::open(path)
            .and_then(|f| f.take(16 * 1024).read_to_end(&mut text))
            .is_ok()
        {
            return String::from_utf8_lossy(&text).to_lowercase().contains("higgsfield");
        }
    }
    false
}

fn file_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["hf.exe", "hf.cmd", "higgsfield.exe", "higgsfield.cmd"]
    } else {
        &["hf", "higgsfield"]
    }
}

pub struct CliSearch<'a> {
    pub override_path: Option<String>,
    pub home: Option<&'a Path>,
    pub appdata: Option<&'a Path>,
    pub local_appdata: Option<&'a Path>,
    pub path_var: Option<OsString>,
}

pub fn locate(search: &CliSearch) -> Option<PathBuf> {
    if let Some(path) = search.override_path.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let path = PathBuf::from(path);
        return (path.is_absolute() && path.is_file()).then_some(path);
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(appdata) = search.appdata {
        dirs.push(appdata.join("npm"));
    }
    if let Some(local) = search.local_appdata {
        dirs.push(local.join("Programs").join("higgsfield").join("bin"));
    }
    if let Some(home) = search.home {
        dirs.push(home.join(".local").join("bin"));
    }
    if let Some(path) = &search.path_var {
        // A relative PATH entry is the working directory's say, not the user's.
        dirs.extend(std::env::split_paths(path).filter(|d| d.is_absolute()));
    }
    dirs.iter()
        .flat_map(|dir| file_names().iter().map(move |name| dir.join(name)))
        .find(|candidate| is_higgsfield(candidate))
}

/// Why a failed run failed, from what it printed. The text is only classified, never shown or kept.
pub fn classify_failure(code: Option<i32>, output: &str) -> Failure {
    let lower = output.to_lowercase();
    if lower.contains("rate limit") || lower.contains("too many requests") || lower.contains("429") {
        return Failure::rate_limited("Higgsfield rate limited this reader. Waiting before retrying.", None);
    }
    if lower.contains("login") || lower.contains("auth") || lower.contains("unauthorized") {
        return Failure::needs_auth("Sign in with `hf auth login`, or add a Higgsfield API key in Settings.");
    }
    Failure::transient(match code {
        Some(code) => format!("The Higgsfield CLI exited with status {code}."),
        None => "The Higgsfield CLI did not finish.".to_string(),
    })
}

fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // A .cmd shim runs under cmd.exe with node as a grandchild. Killing only cmd.exe would leave it.
        let system = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let _ = Command::new(system.join("System32").join("taskkill.exe"))
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Runs `workspace list --json` with no console window, no inherited Hugging Face secrets, bounded
/// output and a hard timeout.
pub fn run_cli(binary: &Path, timeout: Duration) -> Result<String, Failure> {
    let mut command = Command::new(binary);
    command
        .args(["workspace", "list", "--json"])
        .current_dir(std::env::temp_dir())
        .env("TERM", "dumb")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (name, _) in std::env::vars_os() {
        let upper = name.to_string_lossy().to_uppercase();
        if upper.starts_with("HF_") || upper.starts_with("HUGGING_FACE_") || upper.starts_with("HUGGINGFACE_") {
            command.env_remove(&name);
        }
    }
    if let Some(dir) = binary.parent() {
        let mut parts = vec![dir.to_path_buf()];
        if let Some(path) = std::env::var_os("PATH") {
            parts.extend(std::env::split_paths(&path));
        }
        if let Ok(joined) = std::env::join_paths(parts) {
            command.env("PATH", joined);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|_| Failure::transient("The Higgsfield CLI could not be started."))?;
    let (out_tx, out_rx) = mpsc::channel();
    let (err_tx, err_rx) = mpsc::channel();
    if let Some(stream) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream.take(MAX_OUTPUT + 1).read_to_end(&mut bytes);
            let _ = out_tx.send(bytes);
        });
    }
    if let Some(stream) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream.take(MAX_OUTPUT + 1).read_to_end(&mut bytes);
            let _ = err_tx.send(bytes);
        });
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                kill_tree(&mut child);
                return Err(Failure::transient("The Higgsfield CLI did not answer in time."));
            }
        }
    };
    let remaining = deadline.saturating_duration_since(Instant::now()).max(Duration::from_millis(250));
    let stdout = out_rx.recv_timeout(remaining).unwrap_or_default();
    let stderr = err_rx.recv_timeout(Duration::from_millis(250)).unwrap_or_default();
    if !status.success() {
        let printed = format!("{}{}", String::from_utf8_lossy(&stdout), String::from_utf8_lossy(&stderr));
        return Err(classify_failure(status.code(), &printed));
    }
    if stdout.is_empty() || stdout.len() as u64 > MAX_OUTPUT {
        return Err(Failure::transient("The Higgsfield CLI returned no usable output."));
    }
    String::from_utf8(stdout).map_err(|_| Failure::transient("The Higgsfield CLI returned no usable output."))
}

/// `~/.config/higgsfield/credentials.json`, read-only.
pub fn file_token(paths: &[PathBuf]) -> Option<String> {
    paths.iter().find_map(|path| {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .ok()?
            .take(1024 * 1024)
            .read_to_end(&mut bytes)
            .ok()?;
        let root: Value = serde_json::from_slice(&bytes).ok()?;
        ["access_token", "token", "api_key", "key"]
            .iter()
            .find_map(|k| root[*k].as_str().and_then(net::clean_secret))
    })
}

pub fn auth_value(token: &str) -> String {
    if token.contains(':') {
        format!("Key {token}")
    } else {
        format!("Bearer {token}")
    }
}

pub fn fetch_gateway(url: &str, token: &str) -> Result<Reading, Failure> {
    let reply = net::get(
        url,
        &[net::header("Authorization", auth_value(token))],
        net::CLOUD_TIMEOUT,
    )
    .map_err(|_| Failure::transient("Couldn't reach Higgsfield. Check your connection."))?;
    match reply.status {
        200..=299 => {
            let root: Value = serde_json::from_slice(&reply.body)
                .map_err(|_| Failure::transient("Higgsfield returned invalid usage data."))?;
            parse(&root, "api")
        }
        401 => Err(Failure::needs_auth(
            "Higgsfield refused this key. Sign in with `hf auth login` or replace the key in Settings.",
        )),
        429 => Err(Failure::rate_limited(
            "Higgsfield rate limited this reader. Waiting before retrying.",
            reply.retry_after,
        )),
        _ => Err(Failure::transient("Higgsfield could not answer. Trying again later.")),
    }
}

/// CLI first. With no credential saved, the CLI's own failure is the true answer: a timeout there says
/// nothing about being signed out.
pub fn read(
    binary: Option<PathBuf>,
    token: Option<String>,
    gateway: &str,
    timeout: Option<Duration>,
) -> Result<Reading, Failure> {
    let mut cli_failure = None;
    if let Some(binary) = binary {
        let text = run_cli(&binary, timeout.unwrap_or(CLI_TIMEOUT));
        match text.and_then(|t| {
            serde_json::from_str::<Value>(&t)
                .map_err(|_| Failure::transient("The Higgsfield CLI returned no usable output."))
        }) {
            Ok(root) => match parse(&root, "cli") {
                Ok(reading) => return Ok(reading),
                Err(failure) => cli_failure = Some(failure),
            },
            Err(failure) => cli_failure = Some(failure),
        }
    }
    match token {
        Some(token) => fetch_gateway(gateway, &token),
        None => Err(cli_failure.unwrap_or_else(|| {
            Failure::needs_auth("Install the Higgsfield CLI and run `hf auth login`, or add an API key in Settings.")
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers_plus::net::testkit;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("matra-hf-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn selected_workspace_wins_and_plan_table_is_marked_derived() {
        let root = json!([
            {"id":"a","name":"Other","plan_type":"ultra","credits":10,"is_selected":false},
            {"id":"b","name":"Personal","plan_type":"ultra","credits":2777.1,"is_selected":true}
        ]);
        let r = parse(&root, "cli").unwrap();
        assert_eq!(r.windows[0].id, "credits");
        assert!((r.windows[0].used - (3000.0 - 2777.1) / 3000.0).abs() < 1e-9);
        assert!(r.windows[0].derived);
        assert_eq!(r.plan.as_deref(), Some("Ultra"));
        assert_eq!(r.details.as_ref().unwrap()["workspace"], "Personal");
    }

    #[test]
    fn several_workspaces_with_none_selected_is_not_a_reading() {
        let root = json!([{"credits":1},{"credits":2}]);
        assert!(parse(&root, "api").is_err());
        assert!(parse(&json!({"workspaces":[]}), "api").is_err());
        assert!(parse(&json!({"unrelated":1}), "api").is_err());
    }

    #[test]
    fn an_account_stated_allowance_is_not_derived_and_extras_are_counted() {
        let r = parse(&json!({"credits":1500,"credit_limit":1000,"plan":"pro"}), "api").unwrap();
        assert!(!r.windows[0].derived);
        assert_eq!(r.windows[0].used, 0.0);
        assert_eq!(r.windows[1].id, "extra_credits");
        assert_eq!(r.windows[1].count, Some(500));
    }

    #[test]
    fn unknown_plan_gets_no_invented_fraction() {
        let r = parse(&json!({"credits":42.6,"subscription_plan_type":"enterprise_x"}), "api").unwrap();
        assert_eq!(r.windows[0].count, Some(43));
        assert_eq!(r.plan.as_deref(), Some("Enterprise X"));
        let free = parse(&json!({"credits":5,"plan_type":"free"}), "api").unwrap();
        assert_eq!(free.windows[0].count, Some(5));
        assert!(parse(&json!({"plan_type":"pro"}), "api").is_err());
        assert!(parse(&json!({"credits":true,"plan_type":"pro"}), "api").is_err());
    }

    #[test]
    fn overdrawn_is_nothing_left_not_negative() {
        let r = parse(&json!({"credits":-5,"credit_limit":100}), "api").unwrap();
        assert_eq!(r.windows[0].used, 1.0);
    }

    #[test]
    fn hugging_face_hf_is_not_mistaken_for_higgsfield() {
        let dir = temp("collision");
        let hf = dir.join("hf");
        std::fs::write(&hf, "#!/bin/sh\nhuggingface\n").unwrap();
        assert!(!is_higgsfield(&hf));
        let shim_ours = dir.join("hf.cmd");
        std::fs::write(&shim_ours, "@node %dp0%\\node_modules\\@higgsfield\\cli\\bin.js %*").unwrap();
        assert!(is_higgsfield(&shim_ours));
        let shim_theirs = dir.join("x.cmd");
        std::fs::write(&shim_theirs, "@python -m huggingface_hub %*").unwrap();
        assert!(!is_higgsfield(&shim_theirs));
        let named = dir.join("higgsfield");
        std::fs::write(&named, "x").unwrap();
        assert!(is_higgsfield(&named));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn override_path_is_absolute_or_nothing() {
        let search = CliSearch {
            override_path: Some("relative/hf".into()),
            home: None,
            appdata: None,
            local_appdata: None,
            path_var: None,
        };
        assert_eq!(locate(&search), None);
    }

    #[test]
    fn classifier_never_needs_the_text_afterwards() {
        assert_eq!(classify_failure(Some(1), "429 too many").status, "backoff");
        assert_eq!(classify_failure(Some(1), "please login first").status, "needsAuth");
        let other = classify_failure(Some(3), "secret-value-in-output");
        assert_eq!(other.status, "error");
        assert!(!other.note.contains("secret"));
    }

    #[test]
    fn gateway_auth_header_and_statuses() {
        assert_eq!(auth_value("abc"), "Bearer abc");
        assert_eq!(auth_value("id:secret"), "Key id:secret");
        let server = testkit::serve(vec![
            ("/ok", testkit::ok(r#"{"credits":10,"credit_limit":100}"#)),
            ("/denied", testkit::response("401 Unauthorized", "", "")),
            ("/slow", testkit::response("429 Too Many Requests", "Retry-After: 120\r\n", "")),
        ]);
        let r = fetch_gateway(&format!("{}/ok", server.base), "synthetic-token").unwrap();
        assert_eq!(r.windows[0].used, 0.9);
        assert!(server.saw("Authorization: Bearer synthetic-token"));
        assert_eq!(fetch_gateway(&format!("{}/denied", server.base), "t").unwrap_err().status, "needsAuth");
        let slow = fetch_gateway(&format!("{}/slow", server.base), "t").unwrap_err();
        assert_eq!(slow.status, "backoff");
        assert_eq!(slow.retry_after, Some(120));
    }

    #[test]
    fn nothing_available_asks_for_sign_in() {
        assert_eq!(read(None, None, "http://127.0.0.1:1", None).unwrap_err().status, "needsAuth");
    }

    #[test]
    fn credential_file_is_read_by_key_name() {
        let dir = temp("cred");
        let file = dir.join("credentials.json");
        std::fs::write(&file, r#"{"unrelated":"x","api_key":" synthetic "}"#).unwrap();
        assert_eq!(file_token(&[dir.join("missing.json"), file]).as_deref(), Some("synthetic"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn cli_runs_bounded_and_is_killed_on_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("cli");
        let script = |name: &str, body: &str| {
            let path = dir.join(name);
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        let good = script("good", r#"echo '[{"credits":100,"plan_type":"basic","is_selected":true}]'"#);
        let r = read(Some(good), None, "http://127.0.0.1:1", Some(Duration::from_secs(5))).unwrap();
        assert_eq!(r.details.as_ref().unwrap()["source"], "cli");
        let denied = script("denied", "echo 'not logged in, run login' >&2; exit 1");
        let failure = read(Some(denied), None, "http://127.0.0.1:1", Some(Duration::from_secs(5))).unwrap_err();
        assert_eq!(failure.status, "needsAuth");
        let hung = script("hung", "exec sleep 30");
        let started = Instant::now();
        let failure = read(Some(hung), None, "http://127.0.0.1:1", Some(Duration::from_millis(300))).unwrap_err();
        assert!(failure.note.contains("in time"));
        assert!(started.elapsed() < Duration::from_secs(5));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
