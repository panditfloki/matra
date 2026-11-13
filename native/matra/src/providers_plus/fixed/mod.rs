//! Fixed-provider protocols ported from the preserved native Mac clients. No automatic login or refresh.
pub mod amp;
pub mod gemini;
pub mod kiro;
pub mod minimax;
use super::engine::Env;
use super::model::{count_window, number, rfc3339_ms, Failure, Reading};
use super::net::{self, Reply};
use crate::usage::LimitWindow;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const DEVIN_URL: &str = "https://server.self-serve.windsurf.com/exa.seat_management_pb.SeatManagementService/GetUserStatus";
pub const APIFY_URL: &str = "https://api.apify.com/v2/users/me/limits";
pub const KILO_URL: &str = "https://api.kilo.ai";

pub fn absent(note: &str) -> Failure {
    let mut f = Failure::error(note);
    f.status = "none";
    f
}
pub fn invalid(name: &str) -> Failure {
    Failure::error(format!("{name} returned unrecognized usage data."))
}
pub fn numeric(v: &Value) -> Option<f64> {
    number(v).or_else(|| {
        v.as_str()?
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && n.abs() < 1e15)
    })
}
pub fn positive(v: &Value) -> Option<f64> {
    numeric(v).filter(|n| *n >= 0.0)
}
pub fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}
pub fn fraction(id: &str, label: &str, used: f64, reset: Option<u64>) -> LimitWindow {
    LimitWindow {
        id: id.into(),
        label: label.into(),
        used,
        resets_at: reset,
        ..Default::default()
    }
}
pub fn textual(
    id: &str,
    label: &str,
    value: String,
    detail: Option<String>,
    reading: &mut Reading,
) {
    reading.windows.push(count_window(id, label, 0));
    let details = reading.details.get_or_insert_with(|| json!({}));
    if details.get("texts").is_none() {
        details["texts"] = json!({});
    }
    details["texts"][id] = json!({"used_text":value,"detail":detail});
}
pub fn json_file(path: &Path) -> Option<Value> {
    serde_json::from_slice(&read_file(path, 1024 * 1024)?).ok()
}
pub fn read_file(path: &Path, limit: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(limit as u64 + 1)
        .read_to_end(&mut out)
        .ok()?;
    (out.len() <= limit).then_some(out)
}
pub fn database(path: &Path) -> Option<Connection> {
    if !path.is_absolute() || !path.is_file() {
        return None;
    }
    let db = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    db.execute_batch("PRAGMA query_only=ON;").ok()?;
    Some(db)
}
pub fn db_value(path: &Path, table: &str, key: &str) -> Option<Value> {
    if !["ItemTable", "auth_kv", "state"].contains(&table) {
        return None;
    }
    let db = database(path)?;
    let value: String = db
        .query_row(
            &format!("SELECT value FROM {table} WHERE key=?1"),
            [key],
            |r| r.get(0),
        )
        .ok()?;
    serde_json::from_str(&value).ok()
}
pub fn share(env: &Env) -> Option<PathBuf> {
    (env.vars)("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| env.home.as_ref().map(|h| h.join(".local/share")))
}
pub fn http(reply: Reply, name: &str, forbidden_auth: bool) -> Result<Vec<u8>, Failure> {
    match reply.status {
        200..=299 => Ok(reply.body),
        401 => Err(Failure::needs_auth(format!(
            "Sign in to {name} again or replace its token."
        ))),
        403 if forbidden_auth => Err(Failure::needs_auth(format!("{name} refused this login."))),
        403 => Err(Failure::error(format!(
            "This {name} token cannot read account usage. Check its account permissions."
        ))),
        429 => Err(Failure::rate_limited(
            format!("{name} rate limited this reader. Waiting before retrying."),
            reply.retry_after,
        )),
        500..=599 => Err(Failure::transient(format!(
            "{name} could not answer. Trying again later."
        ))),
        _ => Err(Failure::error(format!(
            "{name} returned an unexpected HTTP response."
        ))),
    }
}
pub fn request(
    method: &str,
    url: &str,
    headers: net::Headers,
    body: Option<&str>,
    name: &str,
    forbidden_auth: bool,
) -> Result<Value, Failure> {
    let reply = net::send(method, url, &headers, body, net::CLOUD_TIMEOUT).map_err(|_| {
        Failure::transient(format!("Couldn't reach {name}. Check your connection."))
    })?;
    serde_json::from_slice(&http(reply, name, forbidden_auth)?).map_err(|_| invalid(name))
}

#[derive(Clone)]
pub struct Credential {
    pub token: String,
    pub organization: Option<String>,
    pub source: &'static str,
    pub plan: Option<String>,
}
impl Credential {
    pub fn scope(&self) -> String {
        use sha2::{Digest, Sha256};
        format!(
            "{:x}",
            Sha256::digest(format!(
                "{}\0{}\0{}",
                self.source,
                self.organization.as_deref().unwrap_or(""),
                self.token
            ))
        )
    }
}
pub fn amp_credential(env: &Env) -> Result<Credential, Failure> {
    let p = share(env)
        .ok_or_else(|| Failure::needs_auth("Run amp login to see usage."))?
        .join("amp/secrets.json");
    let bytes = read_file(&p, 1024 * 1024).ok_or_else(|| {
        if !p.exists() {
            Failure::needs_auth("Run amp login to see usage.")
        } else {
            Failure::error(
                "Could not read Amp secrets.json. Check file permissions or run amp login again.",
            )
        }
    })?;
    let root: Value = serde_json::from_slice(&bytes)
        .ok()
        .filter(Value::is_object)
        .ok_or_else(|| Failure::error("Amp secrets.json is invalid. Run amp login again."))?;
    let token = ["apiKey@https://ampcode.com/", "apiKey@https://ampcode.com"]
        .into_iter()
        .find_map(|k| {
            root[k]
                .as_str()
                .and_then(net::clean_secret)
                .filter(|s| !s.chars().any(char::is_whitespace))
        })
        .ok_or_else(|| Failure::needs_auth("Run amp login for ampcode.com to see usage."))?;
    Ok(Credential {
        token,
        organization: None,
        source: "Amp CLI",
        plan: None,
    })
}
pub fn kilo_credential(env: &Env) -> Result<Credential, Failure> {
    let p = share(env)
        .ok_or_else(|| Failure::needs_auth("Sign in with Kilo CLI."))?
        .join("kilo/auth.json");
    let root = json_file(&p).ok_or_else(|| Failure::needs_auth("Sign in with Kilo CLI."))?;
    let row = &root["kilo"];
    let (key, organization) = match row["type"].as_str() {
        Some("oauth") => (
            "access",
            row["accountId"].as_str().and_then(net::clean_secret),
        ),
        Some("api") => ("key", None),
        _ => return Err(Failure::needs_auth("Sign in with Kilo CLI.")),
    };
    let token = row[key]
        .as_str()
        .and_then(net::clean_secret)
        .ok_or_else(|| Failure::needs_auth("Sign in with Kilo CLI."))?;
    Ok(Credential {
        token,
        organization,
        source: "Kilo CLI",
        plan: None,
    })
}
pub fn apify_credential(env: &Env, explicit: Option<String>) -> Result<Credential, Failure> {
    if let Some(token) = explicit {
        return Ok(Credential {
            token,
            organization: None,
            source: "Apify token",
            plan: None,
        });
    }
    let p = env
        .home
        .as_ref()
        .ok_or_else(|| Failure::needs_auth("Set APIFY_TOKEN or paste an Apify API token."))?
        .join(".apify/auth.json");
    let root = json_file(&p)
        .ok_or_else(|| Failure::needs_auth("Run apify login or paste an Apify API token."))?;
    let token=root["token"].as_str().and_then(net::clean_secret).ok_or_else(||Failure::needs_auth("Apify CLI keeps this token in its private keyring. Paste an Apify API token in Settings."))?;
    Ok(Credential {
        token,
        organization: None,
        source: "Apify CLI",
        plan: text(&root["plan"]["id"]).map(|s| {
            let mut c = s.chars();
            c.next()
                .map(|first| first.to_uppercase().to_string() + &c.as_str().to_lowercase())
                .unwrap_or_default()
        }),
    })
}
pub fn devin_credential(env: &Env) -> Result<Credential, Failure> {
    let desktop = env.appdata.as_ref().and_then(|d| {
        db_value(
            &d.join("Devin/User/globalStorage/state.vscdb"),
            "ItemTable",
            "windsurfAuthStatus",
        )
    });
    if let Some(token) = desktop
        .as_ref()
        .and_then(|v| v["apiKey"].as_str())
        .and_then(net::clean_secret)
    {
        return Ok(Credential {
            token,
            organization: None,
            source: "Devin Desktop",
            plan: None,
        });
    }
    let path = share(env)
        .ok_or_else(|| Failure::needs_auth("Run devin auth login."))?
        .join("devin/credentials.toml");
    let content = read_file(&path, 1024 * 1024)
        .ok_or_else(|| Failure::needs_auth("Run devin auth login."))?;
    let content = String::from_utf8_lossy(&content);
    let token = content
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once('=')?;
            if k.trim() != "windsurf_api_key" {
                return None;
            }
            let s = v.trim();
            s.strip_prefix('"')?
                .split('"')
                .next()
                .and_then(net::clean_secret)
        })
        .ok_or_else(|| Failure::needs_auth("Run devin auth login."))?;
    Ok(Credential {
        token,
        organization: None,
        source: "Devin CLI",
        plan: None,
    })
}
pub fn devin_read(url: &str, credential: &Credential) -> Result<Reading, Failure> {
    let body=json!({"metadata":{"apiKey":credential.token,"ideName":"windsurf","ideVersion":"1.108.2","extensionName":"windsurf","extensionVersion":"1.108.2","locale":"en"}}).to_string();
    devin_parse(&request(
        "POST",
        url,
        vec![net::header("Connect-Protocol-Version", "1")],
        Some(&body),
        "Devin",
        false,
    )?)
}
pub fn devin_parse(root: &Value) -> Result<Reading, Failure> {
    let p = root
        .pointer("/userStatus/planStatus")
        .filter(|p| p.is_object())
        .ok_or_else(|| invalid("Devin"))?;
    let mut r = Reading::default();
    for (id, label, hidden) in [
        ("daily", "Daily quota", "hideDailyQuota"),
        ("weekly", "Weekly quota", "hideWeeklyQuota"),
    ] {
        if p[hidden].as_bool() == Some(true) {
            continue;
        }
        let reset = positive(&p[format!("{id}QuotaResetAtUnix")])
            .filter(|v| *v > 0.0)
            .map(|v| (v * 1000.0) as u64);
        let remaining = numeric(&p[format!("{id}QuotaRemainingPercent")])
            .filter(|v| (0.0..=100.0).contains(v))
            .or_else(|| reset.map(|_| 0.0));
        if let Some(remaining) = remaining {
            let mut w = fraction(id, label, (100.0 - remaining) / 100.0, reset);
            w.group = Some("Usage".into());
            r.windows.push(w);
        }
    }
    if let Some(micros) = positive(&p["overageBalanceMicros"]) {
        let usd = (micros / 10000.0).round() / 100.0;
        textual(
            "overage",
            "Extra usage balance",
            format!("${usd:.2}"),
            None,
            &mut r,
        );
    }
    if r.windows.is_empty() {
        Err(invalid("Devin"))
    } else {
        Ok(r)
    }
}
pub fn apify_read(url: &str, credential: &Credential) -> Result<Reading, Failure> {
    let mut r = apify_parse(&request(
        "GET",
        url,
        vec![net::bearer(&credential.token)],
        None,
        "Apify",
        false,
    )?)?;
    r.plan = credential.plan.clone();
    Ok(r)
}
pub fn apify_parse(root: &Value) -> Result<Reading, Failure> {
    let p = &root["data"];
    let used = number(&p["current"]["monthlyUsageUsd"])
        .filter(|n| *n >= 0.0)
        .ok_or_else(|| invalid("Apify"))?;
    let reset = rfc3339_ms(&p["monthlyUsageCycle"]["endAt"]);
    let cap = number(&p["limits"]["maxMonthlyUsageUsd"]).filter(|n| *n > 0.0);
    let mut r = Reading::default();
    let amount = apify_money(used);
    if let Some(cap) = cap {
        r.windows
            .push(fraction("monthly", "Monthly usage", used / cap, reset));
        r.details = Some(
            json!({"texts":{"monthly":{"used_text":amount,"detail":format!("{amount} of {}", apify_money(cap))}}}),
        );
    } else {
        textual(
            "monthly",
            "Monthly usage",
            amount.clone(),
            Some(format!("{amount} this cycle")),
            &mut r,
        );
        r.windows[0].resets_at = reset;
    }
    r.windows[0].duration_ms = rfc3339_ms(&p["monthlyUsageCycle"]["startAt"])
        .zip(reset)
        .filter(|(s, e)| e > s)
        .map(|(s, e)| e - s);
    Ok(r)
}

fn apify_money(value: f64) -> String {
    let plain = format!("{value:.2}");
    let (whole, decimal) = plain.split_once('.').unwrap_or((&plain, "00"));
    let mut out = String::from("$");
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out.push('.');
    out.push_str(decimal);
    out
}

fn kilo_payload(v: &Value) -> Result<&Value, Failure> {
    if v.get("error").is_some() {
        return Err(invalid("Kilo"));
    }
    let data = v.pointer("/result/data").ok_or_else(|| invalid("Kilo"))?;
    Ok(data.get("json").unwrap_or(data))
}
pub fn kilo_windows(v: &Value) -> Result<Vec<LimitWindow>, Failure> {
    let list = kilo_payload(v)?
        .pointer("/subscription/windows")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("Kilo"))?;
    let mut out = Vec::new();
    for w in list {
        let Some(id) = text(&w["id"]) else { continue };
        let Some(rem) = number(&w["remainingPercent"]) else {
            continue;
        };
        let hours = number(&w["period"]["value"])
            .filter(|v| *v > 0.0)
            .and_then(|v| {
                Some(
                    v * match w["period"]["unit"].as_str()? {
                        "hour" => 1.,
                        "day" => 24.,
                        "week" => 168.,
                        "month" => 720.,
                        _ => return None,
                    },
                )
            });
        let label = match hours {
            Some(1.) => "Hourly limit".into(),
            Some(24.) => "Daily limit".into(),
            Some(168.) => "Weekly limit".into(),
            Some(720.) => "Monthly limit".into(),
            Some(h) => format!("{}h limit", h as u64),
            None => "Plan limit".into(),
        };
        let mut window = fraction(
            &id,
            &label,
            (1. - rem / 100.).clamp(0., 1.),
            rfc3339_ms(&w["resetsAt"]),
        );
        window.duration_ms = hours.map(|h| (h * 3600000.) as u64);
        out.push((hours.unwrap_or(f64::MAX), window));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    if out.is_empty() {
        return Err(invalid("Kilo"));
    }
    Ok(out.into_iter().map(|(_, w)| w).collect())
}
pub fn kilo_read(base: &str, credential: &Credential) -> Result<Reading, Failure> {
    let mut headers = vec![net::bearer(&credential.token)];
    if let Some(org) = &credential.organization {
        headers.push(net::header("x-kilocode-organizationid", org));
    }
    let subscriptions = request(
        "GET",
        &format!("{base}/api/trpc/codingPlans.listSubscriptions"),
        headers.clone(),
        None,
        "Kilo",
        true,
    );
    let mut r = Reading::default();
    match subscriptions {
        Ok(v) => {
            let plans = kilo_payload(&v)?
                .as_array()
                .ok_or_else(|| invalid("Kilo"))?;
            for p in plans {
                if !matches!(p["status"].as_str(), Some("active" | "past_due")) {
                    continue;
                }
                let (Some(id), Some(name)) = (text(&p["id"]), text(&p["planName"])) else {
                    continue;
                };
                let mut url = url::Url::parse(&format!("{base}/api/trpc/codingPlans.getUsage"))
                    .map_err(|_| invalid("Kilo"))?;
                url.query_pairs_mut()
                    .append_pair("input", &json!({"subscriptionId":id}).to_string());
                let usage = match request("GET", url.as_str(), headers.clone(), None, "Kilo", true)
                {
                    Ok(v) => v,
                    Err(f) if f.status == "needsAuth" => break,
                    Err(f) => return Err(f),
                };
                let windows = kilo_windows(&usage)?;
                if r.plan.is_none() {
                    r.plan = Some(name);
                    r.windows = windows;
                }
            }
        }
        Err(f) if f.status == "needsAuth" => (),
        Err(f) => return Err(f),
    }
    let balance = request(
        "GET",
        &format!("{base}/api/profile/balance"),
        headers,
        None,
        "Kilo",
        true,
    )?;
    if let Some(value) = number(&balance["balance"]) {
        textual(
            "balance",
            "Credit balance",
            format!("${value:.2}"),
            Some(format!("${value:.2} left")),
            &mut r,
        );
    }
    if r.windows.is_empty() {
        Err(absent("This Kilo account has no usage to read yet."))
    } else {
        Ok(r)
    }
}

#[cfg(test)]
mod tests;
