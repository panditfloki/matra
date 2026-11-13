use super::*;
use regex::Regex;
#[derive(Clone)]
pub struct Targets {
    pub international: Vec<String>,
    pub china: Vec<String>,
    pub cookie_international: String,
    pub cookie_china: String,
}
impl Default for Targets {
    fn default() -> Self {
        Self {
            international: vec![
                "https://api.minimax.io/v1/token_plan/remains".into(),
                "https://www.minimax.io/v1/token_plan/remains".into(),
                "https://api.minimax.io/v1/api/openplatform/coding_plan/remains".into(),
            ],
            china: vec![
                "https://api.minimaxi.com/v1/token_plan/remains".into(),
                "https://www.minimaxi.com/v1/token_plan/remains".into(),
                "https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains".into(),
            ],
            cookie_international: "https://www.minimax.io/v1/api/openplatform/coding_plan/remains"
                .into(),
            cookie_china: "https://www.minimaxi.com/v1/api/openplatform/coding_plan/remains".into(),
        }
    }
}
pub fn coding_key(raw: Option<String>) -> Option<String> {
    let raw = raw?;
    let mut key = raw.trim();
    if key
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("bearer "))
    {
        key = key[7..].trim();
    }
    net::clean_secret(key).filter(|s| !s.to_lowercase().starts_with("sk-api-"))
}
pub fn cookie(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let re=Regex::new(r#"(?i)(?:-H|--header)(?:=|\s+)[\"']Cookie:\s*([^\"']+)[\"']|(?:-b|--cookie)(?:=|\s+)[\"']([^\"']+)[\"']"#).ok()?;
    let candidate = if let Some(c) = re.captures(raw) {
        c.get(1).or_else(|| c.get(2))?.as_str().trim()
    } else if raw
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("cookie:"))
    {
        raw[7..].trim()
    } else {
        if raw.to_lowercase().contains("curl ") || raw.starts_with('-') {
            return None;
        }
        raw
    };
    let lower = candidate.to_lowercase();
    if !candidate.contains('=')
        || [
            "cookies.sqlite",
            "cookies.binarycookies",
            "/chrome/",
            "/chromium/",
            "/safari/",
        ]
        .iter()
        .any(|p| lower.contains(p))
    {
        return None;
    }
    net::clean_secret(candidate)
}
fn envelopes(root: &Value) -> Vec<&Value> {
    vec![&root["base_resp"], &root["data"]["base_resp"]]
}
fn code(v: &Value) -> Option<i64> {
    numeric(&v["status_code"])
        .or_else(|| numeric(&v["code"]))
        .map(|v| v as i64)
}
fn response(reply: Reply, cookie_auth: bool) -> Result<Option<Value>, Failure> {
    let value: Value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
    let codes: Vec<_> = std::iter::once(code(&value))
        .chain(envelopes(&value).into_iter().map(code))
        .flatten()
        .collect();
    if reply.status == 429 || codes.iter().any(|c| [429, 2045].contains(c)) {
        return Err(Failure::rate_limited(
            "MiniMax rate limited this reader.",
            reply.retry_after,
        ));
    }
    if [401, 403].contains(&reply.status)
        || codes.iter().any(|c| [401, 403].contains(c))
        || (cookie_auth && codes.contains(&1004))
    {
        return Err(Failure::needs_auth(
            "MiniMax refused this key or cookie. Check the selected region and sign in again.",
        ));
    }
    if [404, 405].contains(&reply.status) || (!cookie_auth && codes.contains(&1004)) {
        return Ok(None);
    }
    if !(200..300).contains(&reply.status) {
        http(reply, "MiniMax", true)?;
    }
    Ok(Some(value))
}
fn attempt(url: &str, key: Option<&str>, cookie: Option<&str>) -> Result<Option<Value>, Failure> {
    let mut headers = Vec::new();
    if let Some(k) = key {
        headers.push(net::bearer(k));
    }
    if let Some(c) = cookie {
        headers.push(net::header("Cookie", c));
    }
    response(
        net::get(url, &headers, net::CLOUD_TIMEOUT)
            .map_err(|_| Failure::transient("Couldn't reach MiniMax."))?,
        cookie.is_some(),
    )
}
fn key_read(urls: &[String], key: &str, now: u64) -> Result<Reading, Failure> {
    for url in urls {
        if let Some(value) = attempt(url, Some(key), None)? {
            match parse(&value, now) {
                Ok(r) => return Ok(r),
                Err(f) if f.status == "none" || f.note == invalid("MiniMax").note => continue,
                Err(f) => return Err(f),
            }
        }
    }
    Err(absent("MiniMax reported no usage windows."))
}
pub fn read(
    targets: &Targets,
    region: &str,
    key: Option<String>,
    cookie_header: Option<String>,
    now: u64,
) -> Result<Reading, Failure> {
    if let Some(key) = coding_key(key) {
        let result = key_read(
            if region == "china" {
                &targets.china
            } else {
                &targets.international
            },
            &key,
            now,
        );
        return match result {
            Err(f) if f.status == "needsAuth" && region == "international" => {
                key_read(&targets.china, &key, now)
            }
            r => r,
        };
    }
    let c = cookie_header
        .and_then(|s| cookie(&s))
        .ok_or_else(|| Failure::needs_auth("Paste a MiniMax Coding Plan key or console cookie."))?;
    let value = attempt(
        if region == "china" {
            &targets.cookie_china
        } else {
            &targets.cookie_international
        },
        None,
        Some(&c),
    )?
    .ok_or_else(|| invalid("MiniMax"))?;
    let mut r = parse(&value, now)?;
    for w in &mut r.windows {
        w.derived = true;
    }
    Ok(r)
}
fn field<'a>(root: &'a Value, key: &str) -> &'a Value {
    root.get("data")
        .and_then(|v| v.get(key))
        .unwrap_or(&root[key])
}
fn epoch(value: Option<f64>) -> Option<u64> {
    let value = value?;
    if value <= 1e9 {
        return None;
    }
    Some(if value > 1e12 {
        value as u64
    } else {
        (value * 1000.) as u64
    })
}
pub fn parse(root: &Value, now: u64) -> Result<Reading, Failure> {
    if !root.is_object() {
        return Err(invalid("MiniMax"));
    }
    for envelope in envelopes(root) {
        if let Some(c) = code(envelope) {
            if c != 0 && c != 200 {
                let m = text(&envelope["status_msg"])
                    .or_else(|| text(&envelope["msg"]))
                    .unwrap_or_default()
                    .to_lowercase();
                if [1004, 401, 403].contains(&c)
                    || ["cookie", "log in", "login", "unauthorized"]
                        .iter()
                        .any(|s| m.contains(s))
                {
                    return Err(Failure::needs_auth(
                        "MiniMax requires a valid key or cookie.",
                    ));
                }
                return Err(Failure::error("MiniMax rejected the usage request."));
            }
        }
    }
    let mut lanes: Vec<_> = field(root, "model_remains")
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    lanes.sort_by_key(|l| {
        if text(&l["model_name"]).is_some_and(|s| s.eq_ignore_ascii_case("general")) {
            0
        } else {
            1
        }
    });
    for lane in lanes {
        let name = text(&lane["model_name"]).unwrap_or_default().to_lowercase();
        if !(name.is_empty()
            || ["general", "text generation", "text-generation"].contains(&name.as_str())
            || name.contains("minimax-m")
            || name.starts_with("m2."))
        {
            continue;
        }
        let mut r = Reading::default();
        r.plan = ["current_subscribe_title", "plan_name", "combo_title"]
            .into_iter()
            .find_map(|k| text(field(root, k)));
        for (weekly, id, label, prefix) in [
            (false, "session", "5h limit", "current_interval"),
            (true, "weekly", "Weekly limit", "current_weekly"),
        ] {
            let total = numeric(&lane[format!("{prefix}_total_count")]);
            let left = numeric(&lane[format!("{prefix}_usage_count")]);
            let percent = numeric(&lane[format!("{prefix}_remaining_percent")]);
            let status = numeric(&lane[format!("{prefix}_status")]);
            let unlimited = weekly && status == Some(3.) && percent.is_some_and(|p| p >= 100.);
            if !unlimited
                && status == Some(3.)
                && total.unwrap_or(0.) == 0.
                && left.unwrap_or(0.) == 0.
                && percent.is_some_and(|p| p >= 100.)
            {
                continue;
            }
            let used = if unlimited {
                Some(0.)
            } else if let Some(p) = percent {
                Some(((100. - p) / 100.).max(0.))
            } else {
                total
                    .filter(|t| *t > 0.)
                    .zip(left)
                    .map(|(t, l)| (t - l).max(0.) / t)
            };
            let Some(used) = used else { continue };
            let end = epoch(numeric(
                &lane[if weekly {
                    "weekly_end_time"
                } else {
                    "end_time"
                }],
            ));
            let remains = numeric(
                &lane[if weekly {
                    "weekly_remains_time"
                } else {
                    "remains_time"
                }],
            )
            .filter(|v| *v > 0.);
            let reset = if unlimited {
                None
            } else {
                end.filter(|e| *e > now)
                    .or_else(|| remains.map(|v| now.saturating_add(v as u64)))
            };
            let mut window = fraction(id, label, used, reset);
            let start = epoch(numeric(
                &lane[if weekly {
                    "weekly_start_time"
                } else {
                    "start_time"
                }],
            ));
            window.duration_ms = start
                .zip(end)
                .filter(|(s, e)| e > s)
                .map(|(s, e)| e - s)
                .or(Some(if weekly { 7 * 86400000 } else { 5 * 3600000 }));
            let boost = if weekly {
                "weekly_boost_permill"
            } else {
                "interval_boost_permill"
            };
            let counts = if unlimited {
                None
            } else if percent.is_some() {
                numeric(&lane[boost])
                    .or_else(|| numeric(&lane[format!("{boost}e")]))
                    .filter(|b| *b > 0.)
                    .map(|b| {
                        let limit = (b / 10.).round().max(1.);
                        let spent = (used * limit).round();
                        (spent, (limit - spent).max(0.))
                    })
            } else {
                total
                    .filter(|t| *t > 0.)
                    .zip(left)
                    .map(|(t, l)| ((t - l).max(0.), l))
            };
            if let Some((spent, remaining)) = counts {
                let d = r.details.get_or_insert_with(|| json!({"texts":{}}));
                d["texts"][id] = json!({"detail":format!("{} used · {} remaining",spent as i64,remaining as i64),"used_count":spent,"remaining_count":remaining});
            }
            r.windows.push(window);
        }
        if !r.windows.is_empty() {
            return Ok(r);
        }
    }
    Err(absent("MiniMax reported no usage windows."))
}
