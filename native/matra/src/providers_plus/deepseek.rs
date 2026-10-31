//! DeepSeek balance, through its documented API with a key the user supplies.
//!
//! The Mac reads the signed-in platform website inside its own web view, which also gives spend and
//! per-model history. There is no such session to borrow on Windows, so this reader reports what the
//! public balance endpoint answers: the balance, with no denominator, so no ring percentage is invented.
use super::model::{count_window, Failure, Reading};
use super::net;
use chrono::{DateTime, Datelike, Timelike, Utc};
use serde_json::{json, Value};

pub const BALANCE_URL: &str = "https://api.deepseek.com/user/balance";
pub const ENV_KEY: &str = "DEEPSEEK_API_KEY";

/// Peak hours in UTC minutes, Monday to Friday, as the Mac's `DeepSeekPricing.Schedule.current`.
const PEAK_WINDOWS: [(u32, u32); 2] = [(60, 240), (360, 600)];

fn is_peak(at: DateTime<Utc>) -> bool {
    let weekday = at.weekday().number_from_monday();
    let minute = at.hour() * 60 + at.minute();
    weekday <= 5 && PEAK_WINDOWS.iter().any(|(start, end)| *start <= minute && minute < *end)
}

/// The billing phase now and when it next flips. Eight days of minute boundaries always contain a flip.
pub fn pricing_phase(now_ms: u64) -> Value {
    let Some(now) = DateTime::<Utc>::from_timestamp_millis(now_ms as i64) else {
        return Value::Null;
    };
    let name = |peak: bool| if peak { "peak" } else { "offPeak" };
    let current = is_peak(now);
    let midnight = now.timestamp() - i64::from(now.num_seconds_from_midnight());
    for day in 0..=8i64 {
        for boundary in PEAK_WINDOWS.iter().flat_map(|(a, b)| [*a, *b]) {
            let seconds = midnight + day * 86_400 + i64::from(boundary) * 60;
            let Some(candidate) = DateTime::<Utc>::from_timestamp(seconds, 0) else { continue };
            if candidate > now && is_peak(candidate) != current {
                return json!({
                    "phase": name(current),
                    "next_phase": name(!current),
                    "next_at": candidate.timestamp_millis(),
                });
            }
        }
    }
    json!({"phase": name(current), "next_phase": null, "next_at": null})
}

fn decimal(v: &Value) -> Option<f64> {
    v.as_str()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .or_else(|| v.as_f64())
        .filter(|n| n.is_finite())
}

pub fn parse(root: &Value, now: u64) -> Result<Reading, Failure> {
    let infos = root["balance_infos"]
        .as_array()
        .ok_or_else(|| Failure::transient("DeepSeek returned invalid usage data."))?;
    let currencies: Vec<Value> = infos
        .iter()
        .filter_map(|info| {
            Some(json!({
                "currency": info["currency"].as_str()?,
                "total": decimal(&info["total_balance"])?,
                "granted": decimal(&info["granted_balance"]),
                "topped_up": decimal(&info["topped_up_balance"]),
            }))
        })
        .collect();
    let first = currencies
        .first()
        .ok_or_else(|| Failure::error("DeepSeek reported no balance for this key."))?;
    let currency = first["currency"].as_str().unwrap_or_default();
    let total = first["total"].as_f64().unwrap_or(0.0);
    Ok(Reading {
        windows: vec![count_window(
            "balance",
            &format!("Balance ({currency})"),
            total.max(0.0).round() as i64,
        )],
        details: Some(json!({
            "currencies": currencies,
            "is_available": root["is_available"].as_bool(),
            "pricing_phase": pricing_phase(now),
        })),
        ..Default::default()
    })
}

pub fn read(url: &str, key: Option<String>, now: u64) -> Result<Reading, Failure> {
    let key = key.ok_or_else(|| {
        Failure::needs_auth("Add a DeepSeek API key in Settings, or set DEEPSEEK_API_KEY.")
    })?;
    let reply = net::get(url, &[net::bearer(&key)], net::CLOUD_TIMEOUT)
        .map_err(|_| Failure::transient("Couldn't reach DeepSeek. Check your connection."))?;
    match reply.status {
        200 => {
            let root: Value = serde_json::from_slice(&reply.body)
                .map_err(|_| Failure::transient("DeepSeek returned invalid usage data."))?;
            parse(&root, now)
        }
        401 | 403 => Err(Failure::needs_auth("DeepSeek refused this key. Replace it in Settings.")),
        429 => Err(Failure::rate_limited(
            "DeepSeek rate limited this reader. Waiting before retrying.",
            reply.retry_after,
        )),
        _ => Err(Failure::transient("DeepSeek could not answer. Trying again later.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers_plus::net::testkit;

    // Monday 2026-10-26 02:00 UTC.
    const MON_0200: u64 = 1_792_980_000_000;

    #[test]
    fn balance_has_no_invented_denominator() {
        let root = json!({"is_available":true,"balance_infos":[
            {"currency":"CNY","total_balance":"110.40","granted_balance":"10.00","topped_up_balance":"100.40"}]});
        let r = parse(&root, MON_0200).unwrap();
        assert_eq!(r.windows[0].count, Some(110));
        assert_eq!(r.windows[0].used, 0.0);
        assert_eq!(r.windows[0].label, "Balance (CNY)");
        assert_eq!(r.details.as_ref().unwrap()["currencies"][0]["topped_up"], json!(100.40));
    }

    #[test]
    fn empty_or_malformed_is_not_a_zero_balance() {
        assert!(parse(&json!({"balance_infos":[]}), 0).is_err());
        assert!(parse(&json!({"balance_infos":[{"currency":"USD"}]}), 0).is_err());
        assert!(parse(&json!({"error":"x"}), 0).is_err());
    }

    #[test]
    fn peak_schedule_matches_the_published_rule() {
        let p = pricing_phase(MON_0200);
        assert_eq!(p["phase"], "peak");
        assert_eq!(p["next_phase"], "offPeak");
        assert_eq!(p["next_at"], json!(MON_0200 + 2 * 3_600_000));
        // Saturday 2026-10-31 02:00 UTC is off-peak until Monday 01:00.
        let saturday = MON_0200 + 5 * 86_400_000;
        let p = pricing_phase(saturday);
        assert_eq!(p["phase"], "offPeak");
        assert_eq!(p["next_at"], json!(saturday + 47 * 3_600_000));
    }

    #[test]
    fn statuses_are_honest_and_the_key_goes_only_in_the_header() {
        let server = testkit::serve(vec![
            ("/ok", testkit::ok(r#"{"balance_infos":[{"currency":"USD","total_balance":"5"}]}"#)),
            ("/denied", testkit::response("401 Unauthorized", "", "")),
            ("/slow", testkit::response("429 Too Many Requests", "Retry-After: 700\r\n", "")),
            ("/oops", testkit::response("500 Oops", "", "details the user must not see")),
        ]);
        assert_eq!(read("http://127.0.0.1:1", None, 0).unwrap_err().status, "needsAuth");
        let key = || Some("synthetic".to_string());
        assert_eq!(read(&format!("{}/ok", server.base), key(), 0).unwrap().windows[0].count, Some(5));
        assert!(server.saw("Authorization: Bearer synthetic"));
        assert_eq!(read(&format!("{}/denied", server.base), key(), 0).unwrap_err().status, "needsAuth");
        assert_eq!(read(&format!("{}/slow", server.base), key(), 0).unwrap_err().retry_after, Some(700));
        let oops = read(&format!("{}/oops", server.base), key(), 0).unwrap_err();
        assert!(oops.transient);
        assert!(!oops.note.contains("details"));
    }
}
