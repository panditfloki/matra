//! Reconciled local Gemini API token histories. Never reads an API key or sends a request.
use super::*;
use chrono::{Datelike, Local, TimeZone};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Totals {
    pub month: i64,
    pub today: i64,
    pub calls: i64,
}
#[derive(Clone, Copy)]
pub struct Period {
    pub start: i64,
    pub day: i64,
    pub end: i64,
    pub next_day: i64,
}
pub fn period(now: u64) -> Option<Period> {
    let d = Local.timestamp_millis_opt(now as i64).single()?;
    let date = d.date_naive();
    let start = Local
        .from_local_datetime(&date.with_day(1)?.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    let next_month = if d.month() == 12 {
        chrono::NaiveDate::from_ymd_opt(d.year() + 1, 1, 1)?
    } else {
        chrono::NaiveDate::from_ymd_opt(d.year(), d.month() + 1, 1)?
    };
    let day = Local
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    let end = Local
        .from_local_datetime(&next_month.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    let next = Local
        .from_local_datetime(&date.succ_opt()?.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    Some(Period {
        start: start.timestamp_millis(),
        day: day.timestamp_millis(),
        end: end.timestamp_millis(),
        next_day: next.timestamp_millis(),
    })
}
fn add(t: &mut Totals, at: i64, tokens: i64, calls: i64, p: Period) {
    if tokens <= 0 || at < p.start || at >= p.end {
        return;
    }
    t.month = t.month.saturating_add(tokens);
    t.calls = t.calls.saturating_add(calls.max(0));
    if at >= p.day && at < p.next_day {
        t.today = t.today.saturating_add(tokens);
    }
}
pub fn cli_text(text: &str, p: Period) -> Totals {
    let mut records = BTreeMap::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v["type"] != "gemini" || !v["tokens"].is_object() {
            continue;
        }
        let (Some(id), Some(at)) = (self::text(&v["id"]), rfc3339_ms(&v["timestamp"])) else {
            continue;
        };
        let tokens = v["tokens"]["total"].as_i64().unwrap_or(0);
        records.insert(id, (at as i64, tokens));
    }
    let mut t = Totals::default();
    for (_, (at, tokens)) in records {
        add(&mut t, at, tokens, 1, p);
    }
    t
}
pub fn cli(root: &Path, p: Period) -> Option<Totals> {
    let dirs = std::fs::read_dir(root).ok()?;
    let mut t = Totals::default();
    let mut files = 0usize;
    for d in dirs.flatten().take(10000) {
        let Ok(chats) = std::fs::read_dir(d.path().join("chats")) else {
            continue;
        };
        for file in chats.flatten() {
            if file.path().extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            files += 1;
            if files > 10000 {
                return None;
            }
            let bytes = read_file(&file.path(), 64 * 1024 * 1024)?;
            let v = cli_text(&String::from_utf8_lossy(&bytes), p);
            t.month = t.month.saturating_add(v.month);
            t.today = t.today.saturating_add(v.today);
            t.calls = t.calls.saturating_add(v.calls);
        }
    }
    Some(t)
}
pub fn opencode(path: &Path, p: Period) -> Option<Totals> {
    let db = database(path)?;
    let has = |name: &str| {
        db.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |r| r.get::<_, i64>(0),
        )
        .ok()
        .unwrap_or(0)
            > 0
    };
    let v2 = has("session_v2") || (!has("message") && has("session_message"));
    let table = if v2 {
        "session_message"
    } else if has("message") {
        "message"
    } else {
        return None;
    };
    let role = if v2 {
        "type='assistant'"
    } else {
        "json_extract(data,'$.role')='assistant'"
    };
    let sql=format!("SELECT time_created,data FROM {table} WHERE {role} AND COALESCE(json_extract(data,'$.model.providerID'),json_extract(data,'$.providerID'))='google' AND time_created>=?1");
    let mut st = db.prepare(&sql).ok()?;
    let rows = st
        .query_map([p.start], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })
        .ok()?;
    let mut t = Totals::default();
    for row in rows {
        let (at, text) = row.ok()?;
        let v: Value = serde_json::from_str(&text).ok()?;
        let n = |ptr: &str| v.pointer(ptr).and_then(Value::as_i64).unwrap_or(0).max(0);
        let total = n("/tokens/total");
        let tokens = if total > 0 {
            total
        } else {
            [
                "/tokens/input",
                "/tokens/output",
                "/tokens/reasoning",
                "/tokens/cache/read",
                "/tokens/cache/write",
            ]
            .iter()
            .fold(0_i64, |a, k| a.saturating_add(n(k)))
        };
        add(&mut t, at, tokens, 1, p);
    }
    Some(t)
}
pub fn hermes(path: &Path, p: Period) -> Option<Totals> {
    let db = database(path)?;
    let mut st=db.prepare("SELECT last_seen,input_tokens+cache_read_tokens+cache_write_tokens+output_tokens,api_call_count FROM session_model_usage WHERE billing_provider='gemini' AND last_seen>=?1").ok()?;
    let rows = st
        .query_map([p.start / 1000], |r| {
            Ok((
                r.get::<_, f64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .ok()?;
    let mut t = Totals::default();
    for row in rows {
        let (at, tokens, calls) = row.ok()?;
        if at.is_finite() {
            add(&mut t, (at * 1000.) as i64, tokens, calls, p);
        }
    }
    Some(t)
}
pub fn read(env: &Env, budget: Option<u64>, now: u64) -> Result<Reading, Failure> {
    let home = env
        .home
        .as_deref()
        .ok_or_else(|| absent("No local Gemini token histories found."))?;
    let p = period(now).ok_or_else(|| invalid("Gemini API"))?;
    let data = share(env).ok_or_else(|| invalid("Gemini API"))?;
    let sources = [
        ("cli", "Gemini CLI", cli(&home.join(".gemini/tmp"), p)),
        (
            "opencode",
            "OpenCode",
            opencode(&data.join("opencode/opencode.db"), p),
        ),
        (
            "hermes",
            "Hermes",
            hermes(&home.join(".hermes/state.db"), p),
        ),
    ];
    let active: Vec<_> = sources
        .into_iter()
        .filter_map(|(id, name, usage)| usage.map(|u| (id, name, u)))
        .collect();
    if active.is_empty() {
        return Err(absent("No Gemini CLI, OpenCode or Hermes sessions found."));
    }
    let total = active
        .iter()
        .fold(Totals::default(), |a, (_, _, b)| Totals {
            month: a.month.saturating_add(b.month),
            today: a.today.saturating_add(b.today),
            calls: a.calls.saturating_add(b.calls),
        });
    let mut r = Reading {
        note: "Derived from local tool logs; this is not a Google quota or bill.".into(),
        ..Default::default()
    };
    if let Some(b) = budget.filter(|b| *b > 0) {
        r.windows.push(fraction(
            "month",
            "Tokens this month · personal budget",
            total.month as f64 / b as f64,
            Some(p.end as u64),
        ));
    } else {
        let mut w = count_window(
            "month",
            "Tokens this month · billed per token, no limit",
            total.month,
        );
        w.resets_at = Some(p.end as u64);
        r.windows.push(w);
    }
    let mut today = count_window("today", "Tokens today", total.today);
    today.resets_at = Some(p.next_day as u64);
    r.windows.push(today);
    for (id, name, u) in active {
        r.windows
            .push(count_window(id, &format!("{name} · this month"), u.month));
    }
    for w in &mut r.windows {
        w.derived = true;
        if w.id == "month" {
            w.duration_ms = Some((p.end - p.start) as u64);
        } else if w.id == "today" {
            w.duration_ms = Some((p.next_day - p.day) as u64);
        }
    }
    r.details = Some(
        json!({"calls_this_month":total.calls,"local_history":true,"texts":{"month":{"detail":budget.filter(|b|*b>0).map(|b|format!("{} tokens of {b} personal budget",total.month))}}}),
    );
    Ok(r)
}
