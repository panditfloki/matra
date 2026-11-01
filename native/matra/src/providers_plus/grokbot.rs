//! Grok Bot's weekly usage, from the cache the app keeps on this machine.
//!
//! No endpoint is called and no credential is read. The app writes plain JSON into
//! `sand-client-persistence`, one blob per key, the file name being the key in lower-case unpadded
//! base32. The number only moves while Grok Bot runs, so a reading past its own expiry is reported as
//! stale, never as the present truth.
use super::model::{fraction_window, number, Failure, Reading};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const ACCOUNT_SLOT_KEY: &str = "sand.client.slice.client-meta.account-slot";
const ACCOUNT_PREFIX: &str = "sand.client.slice.account.";
const WEEKLY_SUFFIX: &str = ".weekly-usage.cache";
const MAX_BLOB: u64 = 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn nothing_metered() -> Failure {
    Failure::error("Grok Bot has nothing metered on this account yet.")
}

fn no_cache() -> Failure {
    Failure::needs_auth("Install Grok Bot and sign in. Mātrā reads the weekly usage it keeps on this PC.")
}

pub fn base32_encode(data: &[u8]) -> String {
    let mut out = String::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for byte in data {
        buffer = (buffer << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
        buffer &= (1 << bits) - 1;
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut buffer, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        if c == b'=' {
            break;
        }
        let index = ALPHABET.iter().position(|a| *a == c.to_ascii_uppercase())? as u32;
        buffer = (buffer << 5) | index;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 255) as u8);
        }
        buffer &= (1 << bits) - 1;
    }
    Some(out)
}

pub fn blob_filename(key: &str) -> String {
    format!("{}.blob", base32_encode(key.as_bytes()).to_lowercase())
}

pub fn decode_blob_name(name: &str) -> Option<String> {
    String::from_utf8(base32_decode(name.strip_suffix(".blob")?)?).ok()
}

/// Electron keeps app data in %APPDATA% on Windows. Grok Bot's Windows location has not been observed
/// on a real install, so both conventional roots are tried and the first holding a store wins.
pub fn candidate_dirs(appdata: Option<&Path>, local: Option<&Path>) -> Vec<PathBuf> {
    [appdata, local]
        .into_iter()
        .flatten()
        .map(|root| root.join("Grok Bot").join("sand-client-persistence"))
        .collect()
}

fn read_json(path: &Path) -> Option<Value> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_BLOB + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_BLOB {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn modified_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn active_slot(dir: &Path) -> Option<String> {
    let root = read_json(&dir.join(blob_filename(ACCOUNT_SLOT_KEY)))?;
    // The blob's value is the bare string. Anything else is a shape from another version, not the account.
    root["value"].as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

/// Cache keys escape their slot as a URI component; the selected slot is already raw. Decode only
/// the cache slot, once, and reject malformed escapes/UTF-8. This is not form decoding: '+' stays '+'.
fn decode_cache_slot(slot: &str) -> Option<String> {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    let bytes = slot.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex(*bytes.get(index + 1)?)?;
            let low = hex(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn positive_ms(v: &Value) -> Option<u64> {
    number(v).filter(|n| *n > 0.0).map(|n| n as u64)
}

pub fn reading_from(root: &Value, fallback_ms: u64, now: u64) -> Result<Reading, Failure> {
    let value = root
        .get("value")
        .filter(|v| v.is_object())
        .ok_or_else(|| Failure::error("Grok Bot's cache could not be read."))?;
    let reading = &value["reading"];
    let usage = &reading["usage"];
    if value["kind"].as_str() != Some("present") || !usage.is_object() {
        return Err(nothing_metered());
    }
    let percent = number(&usage["percentUsed"]).ok_or_else(nothing_metered)?;
    // A zero on an account with no included limit is the same absence spelled as a number.
    if percent <= 0.0 && usage["hasNonZeroIncludedLimit"].as_bool() == Some(false) {
        return Err(nothing_metered());
    }
    let percent = percent.max(0.0);
    let as_of = positive_ms(&reading["readAtMs"]).unwrap_or(fallback_ms);
    let resets_at = positive_ms(&usage["nextResetMs"]);
    // Nothing on disk may stay fresh forever: no stated expiry means a day after the reading.
    let expires_at = positive_ms(&value["expiresAtMs"]).unwrap_or(as_of.saturating_add(86_400_000));
    let expired = expires_at <= now || resets_at.is_some_and(|r| r <= now);

    let mut windows = vec![fraction_window("weekly", "Weekly usage", percent / 100.0, resets_at)];
    let mut on_demand = Value::Null;
    if let (Some(used), Some(limit)) = (
        number(&usage["onDemand"]["usedCents"]),
        number(&usage["onDemand"]["limitCents"]),
    ) {
        on_demand = json!({"used_cents": used, "limit_cents": limit});
        if limit > 0.0 {
            // Its reset is unknown: the weekly payload carries no timestamp for it.
            windows.push(fraction_window("on_demand", "On demand", used / limit, None));
        }
    }
    Ok(Reading {
        windows,
        plan: super::model::non_empty(&usage["grokPlanLabel"]),
        details: Some(json!({
            "percent_used": percent,
            "as_of": as_of,
            "expires_at": expires_at,
            "on_demand": on_demand,
        })),
        note: if percent >= 100.0 {
            "Weekly usage limit reached.".into()
        } else if expired {
            "Grok Bot's last reading has expired. Open Grok Bot to update it.".into()
        } else {
            String::new()
        },
        status: expired.then_some("stale"),
        as_of: Some(as_of),
        ..Default::default()
    })
}

/// The weekly cache for the selected account or, only when no account is selected at all, the freshest
/// one any account left. A selected slot is binding: its cache being missing or damaged is an answer
/// about that account, and another account's week must not stand in for it.
pub fn load(dir: &Path, now: u64) -> Result<Reading, Failure> {
    let entries = std::fs::read_dir(dir).map_err(|_| no_cache())?;
    let mut caches: Vec<(String, PathBuf, u64)> = entries
        .flatten()
        .take(MAX_ENTRIES)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let key = decode_blob_name(&name)?;
            let slot = key.strip_prefix(ACCOUNT_PREFIX)?.strip_suffix(WEEKLY_SUFFIX)?.to_string();
            let path = entry.path();
            let modified = modified_ms(&path);
            Some((slot, path, modified))
        })
        .collect();
    if caches.is_empty() {
        return Err(no_cache());
    }
    caches.sort_by(|(_, path_a, modified_a), (_, path_b, modified_b)| {
        modified_b.cmp(modified_a).then_with(|| path_a.cmp(path_b))
    });
    if let Some(active) = active_slot(dir) {
        caches.retain(|(slot, _, _)| {
            *slot == active || decode_cache_slot(slot).as_deref() == Some(active.as_str())
        });
        if caches.is_empty() {
            return Err(nothing_metered());
        }
        // The newest alias is authoritative, including an empty or damaged reading.
        caches.truncate(1);
    } else {
        // Keep fallback across different accounts, never to an older alias of an account whose
        // newer cache is empty or damaged.
        let mut seen_slots = std::collections::HashSet::new();
        caches.retain(|(slot, _, _)| {
            seen_slots.insert(decode_cache_slot(slot).unwrap_or_else(|| slot.clone()))
        });
    }
    let mut saw_nothing_metered = false;
    for (_, path, modified) in &caches {
        let Some(root) = read_json(path) else { continue };
        match reading_from(&root, *modified, now) {
            Ok(reading) => return Ok(reading),
            Err(failure) if failure.note == nothing_metered().note => saw_nothing_metered = true,
            Err(_) => {}
        }
    }
    Err(if saw_nothing_metered {
        nothing_metered()
    } else {
        Failure::error("Grok Bot's cache could not be read.")
    })
}

pub fn load_first(dirs: &[PathBuf], now: u64) -> Result<Reading, Failure> {
    match dirs.iter().find(|dir| dir.is_dir()) {
        Some(dir) => load(dir, now),
        None => Err(no_cache()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_791_700_000_000;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("matra-grokbot-{name}-{}-{NOW}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn cache(percent: f64) -> String {
        json!({"schemaVersion":2,"value":{"kind":"present","selectedTeamId":null,
            "reading":{"usage":{"percentUsed":percent,"nextResetMs":NOW + 500_000_000,"isSandTrial":false,
            "hasNonZeroIncludedLimit":true,"isTeamSeat":false,"onDemand":null,"grokPlanLabel":"SuperGrok Plus"},
            "readAtMs":NOW - 1000},"expiresAtMs":NOW + 80_000_000}})
        .to_string()
    }

    fn weekly_name(slot: &str) -> String {
        blob_filename(&format!("{ACCOUNT_PREFIX}{slot}{WEEKLY_SUFFIX}"))
    }

    fn select_slot(dir: &Path, slot: &str) {
        std::fs::write(dir.join(blob_filename(ACCOUNT_SLOT_KEY)), json!({"value":slot}).to_string())
            .unwrap();
    }

    fn write_weekly_at(dir: &Path, slot: &str, body: &str, modified: u64) {
        let path = dir.join(weekly_name(slot));
        std::fs::write(&path, body).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(
                UNIX_EPOCH + std::time::Duration::from_millis(modified),
            ))
            .unwrap();
    }

    #[test]
    fn base32_matches_the_documented_file_name() {
        assert_eq!(
            blob_filename(ACCOUNT_SLOT_KEY),
            "onqw4zbomnwgszlooqxhg3djmnss4y3mnfsw45bnnvsxiyjomfrwg33vnz2c243mn52a.blob"
        );
        assert_eq!(decode_blob_name(&blob_filename("sand.x")).as_deref(), Some("sand.x"));
        assert_eq!(decode_blob_name("not base32!.blob"), None);
        assert_eq!(decode_blob_name("nosuffix"), None);
    }

    #[test]
    fn parses_a_recorded_shape() {
        let root: Value = serde_json::from_str(&cache(85.898948)).unwrap();
        let r = reading_from(&root, 0, NOW).unwrap();
        assert!((r.windows[0].used - 0.85898948).abs() < 1e-9);
        assert_eq!(r.plan.as_deref(), Some("SuperGrok Plus"));
        assert_eq!(r.status, None);
        assert_eq!(r.as_of, Some(NOW - 1000));
    }

    #[test]
    fn past_expiry_is_stale_and_over_100_is_kept_in_details() {
        let mut root: Value = serde_json::from_str(&cache(130.0)).unwrap();
        root["value"]["expiresAtMs"] = json!(NOW - 1);
        let r = reading_from(&root, 0, NOW).unwrap();
        assert_eq!(r.status, Some("stale"));
        assert_eq!(r.windows[0].used, 1.0);
        assert_eq!(r.details.unwrap()["percent_used"], json!(130.0));
    }

    #[test]
    fn missing_expiry_gets_a_day_not_forever() {
        let mut root: Value = serde_json::from_str(&cache(10.0)).unwrap();
        root["value"].as_object_mut().unwrap().remove("expiresAtMs");
        root["value"]["reading"]["readAtMs"] = json!(NOW - 86_400_001);
        root["value"]["reading"]["usage"]["nextResetMs"] = json!(NOW + 1000);
        assert_eq!(reading_from(&root, 0, NOW).unwrap().status, Some("stale"));
    }

    #[test]
    fn null_reading_and_empty_zero_are_not_a_week_unspent() {
        let root = json!({"value":{"kind":"absent"}});
        assert!(reading_from(&root, 0, NOW).is_err());
        let root = json!({"value":{"kind":"present","reading":{"usage":{"percentUsed":0,"hasNonZeroIncludedLimit":false}}}});
        assert!(reading_from(&root, 0, NOW).is_err());
        let root = json!({"value":{"kind":"present","reading":{"usage":{"percentUsed":true}}}});
        assert!(reading_from(&root, 0, NOW).is_err());
    }

    #[test]
    fn on_demand_rides_along_only_with_a_limit() {
        let mut root: Value = serde_json::from_str(&cache(10.0)).unwrap();
        root["value"]["reading"]["usage"]["onDemand"] = json!({"usedCents":250,"limitCents":1000});
        let r = reading_from(&root, 0, NOW).unwrap();
        assert_eq!(r.windows[1].id, "on_demand");
        assert_eq!(r.windows[1].used, 0.25);
        root["value"]["reading"]["usage"]["onDemand"] = json!({"usedCents":0,"limitCents":0});
        assert_eq!(reading_from(&root, 0, NOW).unwrap().windows.len(), 1);
    }

    #[test]
    fn the_selected_account_is_binding() {
        let dir = temp("selected");
        std::fs::write(dir.join(weekly_name("grok|user_b")), cache(70.0)).unwrap();
        std::fs::write(
            dir.join(blob_filename(ACCOUNT_SLOT_KEY)),
            json!({"value":"grok|user_a"}).to_string(),
        )
        .unwrap();
        // Selected account has no cache: user_b's number must not stand in.
        assert!(load(&dir, NOW).unwrap_err().note.contains("nothing metered"));
        std::fs::write(dir.join(weekly_name("grok|user_a")), cache(20.0)).unwrap();
        assert!((load(&dir, NOW).unwrap().windows[0].used - 0.2).abs() < 1e-9);
        // A damaged cache for the selected account is an answer about that account.
        std::fs::write(dir.join(weekly_name("grok|user_a")), "{").unwrap();
        assert!(load(&dir, NOW).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_raw_selected_slot_matches_an_escaped_cache_key() {
        let dir = temp("escaped-selected");
        write_weekly_at(&dir, "grok%7Cuser_active", &cache(10.0), NOW - 2000);
        write_weekly_at(&dir, "grok%7Cuser_other", &cache(90.0), NOW - 1000);
        select_slot(&dir, "grok|user_active");

        assert!((load(&dir, NOW).unwrap().windows[0].used - 0.1).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_escaped_foreign_account_cannot_fill_the_selected_slot() {
        let dir = temp("escaped-foreign");
        std::fs::write(dir.join(weekly_name("grok%7Cuser_other")), cache(90.0)).unwrap();
        select_slot(&dir, "grok|user_active");

        assert!(load(&dir, NOW).unwrap_err().note.contains("nothing metered"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_slot_escapes_are_decoded_only_once() {
        let dir = temp("escaped-once");
        std::fs::write(dir.join(weekly_name("grok%257Cuser_active")), cache(90.0)).unwrap();
        select_slot(&dir, "grok|user_active");
        assert!(load(&dir, NOW).is_err());

        // The active slot remains a literal value; a cache may escape its literal percent.
        select_slot(&dir, "grok%7Cuser_active");
        assert!((load(&dir, NOW).unwrap().windows[0].used - 0.9).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_selected_slot_is_not_uri_decoded() {
        let dir = temp("selected-literal");
        std::fs::write(dir.join(weekly_name("grok|user_active")), cache(90.0)).unwrap();
        select_slot(&dir, "grok%7Cuser_active");

        assert!(load(&dir, NOW).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_cache_slot_escapes_are_not_partially_decoded() {
        let dir = temp("escaped-malformed");
        for suffix in ["%", "%7", "%GG", "%FF"] {
            let cache_slot = format!("grok%7Cuser_active{suffix}");
            std::fs::write(dir.join(weekly_name(&cache_slot)), cache(90.0)).unwrap();
            select_slot(&dir, &format!("grok|user_active{suffix}"));

            assert_eq!(decode_cache_slot(&cache_slot), None);
            assert!(load(&dir, NOW).is_err(), "{suffix}");
        }
        assert_eq!(decode_cache_slot("grok%7cuser+name").as_deref(), Some("grok|user+name"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_newest_raw_or_escaped_alias_wins() {
        let dir = temp("alias-freshness");
        select_slot(&dir, "grok|user_active");
        for (older, newer) in [
            ("grok|user_active", "grok%7Cuser_active"),
            ("grok%7Cuser_active", "grok|user_active"),
        ] {
            write_weekly_at(&dir, older, &cache(10.0), NOW - 2000);
            write_weekly_at(&dir, newer, &cache(80.0), NOW - 1000);

            assert!((load(&dir, NOW).unwrap().windows[0].used - 0.8).abs() < 1e-9);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn alias_ties_use_a_stable_filename_order() {
        let dir = temp("alias-ties");
        write_weekly_at(&dir, "grok|user_active", &cache(10.0), NOW - 1000);
        write_weekly_at(&dir, "grok%7Cuser_active", &cache(80.0), NOW - 1000);
        select_slot(&dir, "grok|user_active");

        // These fixed blob names sort the escaped key first at equal timestamps.
        assert!((load(&dir, NOW).unwrap().windows[0].used - 0.8).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_newer_empty_or_damaged_alias_cannot_revive_an_older_reading() {
        let dir = temp("alias-retired");
        select_slot(&dir, "grok|user_active");
        for (older, newer) in [
            ("grok|user_active", "grok%7Cuser_active"),
            ("grok%7Cuser_active", "grok|user_active"),
        ] {
            for body in [r#"{"value":{"kind":"absent"}}"#, "{broken"] {
                write_weekly_at(&dir, older, &cache(10.0), NOW - 2000);
                write_weekly_at(&dir, newer, body, NOW - 1000);

                assert!(load(&dir, NOW).is_err(), "{body}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_selected_slot_a_newer_alias_still_retires_the_older_reading() {
        let dir = temp("alias-no-selection");
        write_weekly_at(&dir, "grok|user_active", &cache(10.0), NOW - 2000);
        write_weekly_at(&dir, "grok|user_other", &cache(30.0), NOW - 3000);
        for body in [r#"{"value":{"kind":"absent"}}"#, "{broken"] {
            write_weekly_at(&dir, "grok%7Cuser_active", body, NOW - 1000);

            assert!((load(&dir, NOW).unwrap().windows[0].used - 0.3).abs() < 1e-9);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_slot_uses_what_can_be_read() {
        let dir = temp("noslot");
        std::fs::write(dir.join(weekly_name("grok|user_c")), "{broken").unwrap();
        std::fs::write(dir.join(weekly_name("grok|user_d")), cache(33.0)).unwrap();
        assert!((load(&dir, NOW).unwrap().windows[0].used - 0.33).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn absent_store_asks_for_the_app_not_an_error() {
        let dir = temp("absent");
        let missing = dir.join("nope");
        assert_eq!(load(&missing, NOW).unwrap_err().status, "needsAuth");
        assert_eq!(load(&dir, NOW).unwrap_err().status, "needsAuth");
        assert_eq!(load_first(&[missing], NOW).unwrap_err().status, "needsAuth");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn windows_store_locations_are_checked_in_order() {
        let a = PathBuf::from("/a");
        let l = PathBuf::from("/l");
        let dirs = candidate_dirs(Some(&a), Some(&l));
        assert_eq!(dirs[0], a.join("Grok Bot").join("sand-client-persistence"));
        assert_eq!(dirs[1], l.join("Grok Bot").join("sand-client-persistence"));
    }
}
