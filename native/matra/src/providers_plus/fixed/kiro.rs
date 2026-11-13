//! Kiro owns login/rotation. `/usage` is primary; failed enrichment never erases its reading.
use super::*;
use chrono::{Datelike, Local, TimeZone};
use regex::Regex;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn captures(pattern: &str, text: &str) -> Option<Vec<String>> {
    let re = Regex::new(pattern).ok()?;
    let c = re.captures(text)?;
    Some(
        c.iter()
            .skip(1)
            .map(|m| m.map_or("", |m| m.as_str()).trim().to_string())
            .collect(),
    )
}
pub fn strip_ansi(text: &str) -> String {
    Regex::new("\x1b\\[[0-9;:?]*[ -/]*[@-~]|\x1b\\][^\x07\x1b]*(?:\x07|\x1b\\\\)|\x1b[()][0-9A-Za-z]|\x1b[@-Z\\\\^_]").unwrap().replace_all(text,"").into_owned()
}
fn reset(text: &str, now: u64) -> Option<u64> {
    let f = captures(r"resets on (\d{4}-\d{2}-\d{2}|\d{1,2}/\d{1,2})", text)?;
    let stamp = &f[0];
    let now = Local.timestamp_millis_opt(now as i64).single()?;
    let date = if stamp.contains('-') {
        chrono::NaiveDate::parse_from_str(stamp, "%Y-%m-%d").ok()?
    } else {
        let (m, d) = stamp.split_once('/')?;
        let m = m.parse().ok()?;
        let d = d.parse().ok()?;
        let mut date = chrono::NaiveDate::from_ymd_opt(now.year(), m, d)?;
        if date < now.date_naive() {
            date = chrono::NaiveDate::from_ymd_opt(now.year() + 1, m, d)?;
        }
        date
    };
    u64::try_from(
        Local
            .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
            .earliest()?
            .timestamp_millis(),
    )
    .ok()
}
pub fn parse(output: &str, now: u64) -> Result<Reading, Failure> {
    let text = strip_ansi(output);
    let lower = text.to_lowercase();
    if [
        "not logged in",
        "login required",
        "failed to initialize auth portal",
        "kiro-cli login",
        "oauth error",
    ]
    .iter()
    .any(|s| lower.contains(s))
    {
        return Err(Failure::needs_auth("Run kiro-cli login to see usage."));
    }
    let plan = [
        r"Plan:[ \t]*([^|\r\n]+?)[ \t]*\|[ \t]*[0-9]+[ \t]+usage breakdowns?",
        r"Estimated Usage[ \t]*\|[^\n|]*\|[ \t]*([A-Z][A-Z0-9+ ]+)",
        r"\|[ \t]*(KIRO(?:[ \t]+[A-Za-z0-9+]+)+)",
        r"Plan:[ \t]*([^|\r\n]+)",
    ]
    .into_iter()
    .find_map(|p| captures(p, &text).map(|c| c[0].clone()));
    let percent = captures(r"█+\s*(\d+)%", &text).and_then(|c| c[0].parse::<f64>().ok());
    let pair = captures(r"\((\d+\.?\d*)\s+of\s+(\d+)\s+covered", &text)
        .and_then(|c| Some((c[0].parse::<f64>().ok()?, c[1].parse::<f64>().ok()?)));
    let used = percent
        .map(|p| p / 100.)
        .or_else(|| pair.filter(|(_, total)| *total > 0.).map(|(u, t)| u / t))
        .filter(|n| n.is_finite());
    let bonus = captures(r"Bonus credits:\s*(\d+\.?\d*)/(\d+)", &text)
        .and_then(|c| Some((c[0].parse::<f64>().ok()?, c[1].parse::<f64>().ok()?)));
    if plan.is_none() && used.is_none() && bonus.is_none() {
        return Err(absent("Kiro CLI reported no usage."));
    }
    let mut r = Reading {
        plan: plan.map(|p| {
            if p.to_lowercase().contains("kiro") {
                p.split_whitespace()
                    .map(|w| {
                        let mut c = w.chars();
                        c.next()
                            .map(|f| f.to_uppercase().to_string() + &c.as_str().to_lowercase())
                            .unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                p
            }
        }),
        ..Default::default()
    };
    if let Some(used) = used {
        let reset = reset(&text, now);
        let mut w = fraction("credits", "Credits", used, reset);
        w.duration_ms = (reset.is_some() || lower.contains("monthly")).then_some(30 * 86400000);
        r.windows.push(w);
    }
    if let Some((used, total)) = bonus.filter(|(u, t)| u.is_finite() && t.is_finite() && *t > 0.) {
        let days =
            captures(r"expires in (\d+) days?", &text).and_then(|c| c[0].parse::<u64>().ok());
        let mut w = fraction(
            "bonus",
            "Credits",
            used / total,
            days.map(|d| now.saturating_add(d.saturating_mul(86400000))),
        );
        w.group = Some("Bonus".into());
        r.windows.push(w);
    }
    if let Some(flag) = captures(r"(?i)Overages:\s*([^\n]+)", &text) {
        let lower = flag[0].to_lowercase();
        if lower.starts_with("disabled") || lower.starts_with("enabled") {
            textual(
                "overage-status",
                "Overages",
                if lower.starts_with("enabled") {
                    "Enabled"
                } else {
                    "Disabled"
                }
                .into(),
                None,
                &mut r,
            );
        }
    }
    Ok(r)
}
pub fn state_path(env: &Env) -> Option<PathBuf> {
    (env.vars)("KIRO_DATA_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .map(|p| p.join("data.sqlite3"))
        .or_else(|| {
            if cfg!(windows) {
                env.local_appdata
                    .as_ref()
                    .map(|p| p.join("kiro-cli/data.sqlite3"))
            } else {
                share(env).map(|p| p.join("kiro-cli/data.sqlite3"))
            }
        })
}
pub fn endpoint(arn: &str) -> Option<&'static str> {
    let p: Vec<_> = arn.splitn(6, ':').collect();
    if p.len() != 6
        || p[0] != "arn"
        || p[1] != "aws"
        || p[2] != "codewhisperer"
        || p[4].is_empty()
        || !p[5].starts_with("profile/")
        || p[5].len() <= 8
        || arn.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    match p[3] {
        "us-east-1" => Some("https://codewhisperer.us-east-1.amazonaws.com/"),
        "eu-central-1" => Some("https://q.eu-central-1.amazonaws.com/"),
        _ => None,
    }
}
pub fn credential(env: &Env) -> Option<(String, String)> {
    let path = state_path(env)?;
    let token = db_value(&path, "auth_kv", "kirocli:odic:token")?;
    let arn = db_value(&path, "state", "api.codewhisperer.profile")?;
    let token = token["access_token"]
        .as_str()
        .or_else(|| token["accessToken"].as_str())
        .and_then(net::clean_secret)?;
    let arn = text(&arn["arn"])?;
    endpoint(&arn)?;
    Some((token, arn))
}
fn runnable(p: &Path) -> bool {
    if !p.is_absolute() || !p.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0);
    }
    #[cfg(not(unix))]
    {
        true
    }
}
pub fn locate(env: &Env) -> Option<PathBuf> {
    if let Some(path) = (env.vars)("KIRO_CLI_PATH") {
        let p = PathBuf::from(path.trim());
        return runnable(&p).then_some(p);
    }
    let name = if cfg!(windows) {
        "kiro-cli.exe"
    } else {
        "kiro-cli"
    };
    let mut candidates = Vec::new();
    if let Some(h) = &env.home {
        candidates.push(h.join(".local/bin").join(name));
    }
    if !cfg!(windows) {
        candidates.extend([
            PathBuf::from("/usr/local/bin/kiro-cli"),
            PathBuf::from("/opt/homebrew/bin/kiro-cli"),
        ]);
    }
    if let Some(path) = &env.path_var {
        candidates.extend(
            std::env::split_paths(path)
                .filter(|p| p.is_absolute())
                .map(|p| p.join(name)),
        );
    }
    candidates.into_iter().find(|p| runnable(p))
}
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        unsafe {
            kill(-(child.id() as i32), 9);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let system = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let _ = Command::new(system.join("System32/taskkill.exe"))
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .creation_flags(0x0800_0000)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}
pub fn run(binary: &Path, cwd: &Path, timeout: Duration) -> Result<String, Failure> {
    std::fs::create_dir_all(cwd)
        .map_err(|_| Failure::error("Could not create the isolated Kiro polling directory."))?;
    let mut cmd = Command::new(binary);
    cmd.args(["chat", "--no-interactive", "/usage"])
        .env("TERM", "dumb")
        .env("KIRO_CHAT_UI", "classic")
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd
        .spawn()
        .map_err(|_| Failure::transient("Kiro CLI could not be started."))?;
    let (out_tx, out_rx) = mpsc::channel();
    let (err_tx, err_rx) = mpsc::channel();
    for (stream, tx) in [
        (
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            out_tx,
        ),
        (
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            err_tx,
        ),
    ] {
        if let Some(s) = stream {
            std::thread::spawn(move || {
                let mut bytes = Vec::new();
                let _ = s.take(65537).read_to_end(&mut bytes);
                let _ = tx.send(bytes);
            });
        }
    }
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                kill_tree(&mut child);
                return Err(Failure::transient("Kiro CLI did not answer in time."));
            }
        }
    };
    let out = out_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    let err = err_rx.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    if out.is_err() || err.is_err() {
        kill_tree(&mut child);
        return Err(Failure::transient("Kiro CLI left an output pipe open."));
    }
    let (out, err) = (out.unwrap(), err.unwrap());
    if out.len() > 65536 || err.len() > 65536 {
        kill_tree(&mut child);
        return Err(Failure::error("Kiro CLI output exceeded the usage limit."));
    }
    if !status.success() {
        return Err(Failure::needs_auth(
            "Kiro CLI declined the usage request. Run kiro-cli login.",
        ));
    }
    let out = String::from_utf8_lossy(&out);
    let err = String::from_utf8_lossy(&err);
    if out.contains("Estimated Usage") {
        Ok(out.into_owned())
    } else if err.contains("Estimated Usage") {
        Ok(err.into_owned())
    } else if !out.trim().is_empty() {
        Ok(out.into_owned())
    } else if !err.trim().is_empty() {
        Ok(err.into_owned())
    } else {
        Err(absent("Kiro CLI reported no usage."))
    }
}
pub fn apply_limits(r: &mut Reading, v: &Value) -> Result<(), Failure> {
    let credits: Vec<_> = v["usageBreakdownList"]
        .as_array()
        .ok_or_else(|| invalid("Kiro"))?
        .iter()
        .filter(|r| r["resourceType"] == "CREDIT")
        .collect();
    if credits.len() != 1 {
        return Err(invalid("Kiro"));
    }
    let c = credits[0];
    // JSONDecoder on the native client rejects malformed present numbers, rather than treating
    // them as absent and silently falling back to an integer or zero.
    for field in [
        "usageLimitWithPrecision",
        "usageLimit",
        "currentUsageWithPrecision",
        "currentUsage",
        "currentOveragesWithPrecision",
        "currentOverages",
        "overageCapWithPrecision",
        "overageCap",
        "overageCharges",
        "nextDateReset",
    ] {
        if c.get(field)
            .is_some_and(|v| !v.is_null() && number(v).is_none())
        {
            return Err(invalid("Kiro"));
        }
    }
    if c.get("bonuses")
        .is_some_and(|v| !v.is_null() && !v.is_array())
    {
        return Err(invalid("Kiro"));
    }
    let n = |precise: &str, raw: &str| number(&c[precise]).or_else(|| number(&c[raw]));
    let limit = n("usageLimitWithPrecision", "usageLimit")
        .filter(|n| *n >= 0.)
        .ok_or_else(|| invalid("Kiro"))?;
    let total = n("currentUsageWithPrecision", "currentUsage")
        .filter(|n| *n >= 0.)
        .ok_or_else(|| invalid("Kiro"))?;
    let over = n("currentOveragesWithPrecision", "currentOverages").unwrap_or(0.);
    if over < 0. || total < over {
        return Err(invalid("Kiro"));
    }
    let used = total - over;
    let bonus = c["bonuses"].as_array().is_some_and(|v| !v.is_empty());
    if !bonus && used > limit {
        return Err(invalid("Kiro"));
    }
    let enabled = v["overageConfiguration"]["overageStatus"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("ENABLED"));
    let cap = if enabled {
        n("overageCapWithPrecision", "overageCap")
    } else {
        None
    };
    if cap.is_some_and(|v| v < 0.) {
        return Err(invalid("Kiro"));
    }
    let reset = number(&c["nextDateReset"])
        .or_else(|| number(&v["nextDateReset"]))
        .filter(|n| (1e9..=4102444800.).contains(n))
        .map(|n| (n * 1000.) as u64);
    if !bonus && limit > 0. {
        let old = r
            .windows
            .iter()
            .find(|w| w.id == "credits")
            .and_then(|w| w.resets_at);
        let mut w = fraction("credits", "Credits", used / limit, reset.or(old));
        w.duration_ms = reset.or(old).map(|_| 30 * 86400000);
        if let Some(at) = r.windows.iter().position(|w| w.id == "credits") {
            r.windows[at] = w;
        } else {
            r.windows.insert(0, w);
        }
    }
    if let Some(cap) = cap.filter(|n| *n > 0.) {
        let w = fraction("overage", "Overage", over / cap, reset);
        if let Some(at) = r.windows.iter().position(|w| w.id == "overage") {
            r.windows[at] = w;
        } else {
            r.windows.push(w);
        }
    }
    Ok(())
}
pub fn read(env: &Env, now: u64) -> Result<Reading, Failure> {
    let binary = locate(env)
        .ok_or_else(|| absent("Kiro CLI is not installed. Install it and run kiro-cli login."))?;
    let mut r = parse(
        &run(
            &binary,
            &env.settings_dir.join("kiro-poll"),
            Duration::from_secs(20),
        )?,
        now,
    )?;
    if let Some((token, arn)) = credential(env) {
        use sha2::{Digest, Sha256};
        let scope = format!("{:x}", Sha256::digest(arn.as_bytes()));
        let state = env.settings_dir.join("kiro-enrichment-backoff.json");
        let persisted = json_file(&state).filter(|v| v["scope"].as_str() == Some(scope.as_str()));
        let until = persisted
            .as_ref()
            .and_then(|v| v["until"].as_u64())
            .unwrap_or(0);
        if now >= until {
            let url = match endpoint(&arn) {
                Some("https://codewhisperer.us-east-1.amazonaws.com/") => &env.urls.kiro_us,
                Some("https://q.eu-central-1.amazonaws.com/") => &env.urls.kiro_eu,
                _ => return Ok(r),
            };
            let body = json!({"profileArn":arn}).to_string();
            let reply = net::send(
                "POST",
                url,
                &[
                    net::bearer(&token),
                    net::header("Content-Type", "application/x-amz-json-1.0"),
                    net::header("X-Amz-Target", "AmazonCodeWhispererService.GetUsageLimits"),
                ],
                Some(&body),
                Duration::from_secs(10),
            );
            if let Ok(reply) = reply {
                if reply.status == 429 {
                    let attempts = persisted
                        .as_ref()
                        .and_then(|v| v["attempts"].as_u64())
                        .unwrap_or(0)
                        .min(u32::MAX as u64 - 1) as u32;
                    let wait = net::backoff_secs(attempts, reply.retry_after);
                    let pending = state.with_extension("pending");
                    if std::fs::write(&pending,json!({"scope":scope,"until":now.saturating_add(wait.saturating_mul(1000)),"attempts":attempts+1}).to_string()).is_ok(){let _=std::fs::rename(&pending,&state);}
                } else if (200..300).contains(&reply.status) {
                    if let Ok(v) = serde_json::from_slice(&reply.body) {
                        let mut enriched = r.clone();
                        if apply_limits(&mut enriched, &v).is_ok() {
                            r = enriched;
                            let _ = std::fs::remove_file(&state);
                        }
                    }
                }
            }
        }
    }
    Ok(r)
}
