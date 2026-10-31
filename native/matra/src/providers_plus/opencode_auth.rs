//! The sign-in OpenCode itself stores, borrowed read-only for its usage endpoint.
//!
//! Two stores, database first. OpenCode 2.x rotates the Go key into `opencode.db` and leaves the copy in
//! `auth.json` answering 401, so the file must never shadow a database that has a row. Only OpenCode's
//! own two integration ids are claimed: another row is another vendor's key.
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const GO_ENDPOINT: &str = "https://opencode.ai/zen/go/v1/usage";
pub const OAUTH_ENDPOINT: &str = "https://opencode.ai/inference/go/v1/usage";
pub const DEFAULT_CONSOLE: &str = "https://opencode.ai/console";

#[derive(Clone, PartialEq)]
pub struct Credential {
    pub token: String,
    pub oauth: bool,
    pub console: Option<String>,
    pub org: Option<String>,
    pub expires: Option<u64>,
}

// Written by hand so a credential can never reach a log through `{:?}`.
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential").field("oauth", &self.oauth).finish_non_exhaustive()
    }
}

impl Credential {
    /// Each route refuses the other's credential, with the same 401 a planless account gets.
    pub fn endpoint(&self) -> &'static str {
        if self.oauth {
            OAUTH_ENDPOINT
        } else {
            GO_ENDPOINT
        }
    }

    /// The console that issued this token. Only an https URL on OpenCode's own domain is believed.
    pub fn console_url(&self) -> String {
        let trusted = self.console.as_deref().filter(|text| {
            url::Url::parse(text).is_ok_and(|u| {
                let host = u.host_str().unwrap_or_default().to_ascii_lowercase();
                u.scheme() == "https" && (host == "opencode.ai" || host.ends_with(".opencode.ai"))
            })
        });
        trusted.unwrap_or(DEFAULT_CONSOLE).trim_end_matches('/').to_string()
    }
}

fn clean(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 8192 && !s.contains(['\r', '\n']))
        .map(str::to_owned)
}

/// A bare string, `{"type":"api","key":...}`, or
/// `{"type":"oauth","access":...,"expires":...,"metadata":{"orgID","server"}}`.
pub fn from_value(entry: &Value) -> Option<Credential> {
    let plain = |token: String| Credential {
        token,
        oauth: false,
        console: None,
        org: None,
        expires: None,
    };
    if let Some(token) = clean(entry.as_str()) {
        return Some(plain(token));
    }
    if !entry.is_object() {
        return None;
    }
    if entry["type"].as_str() == Some("oauth") {
        // `expires` is a number in auth.json and a string in opencode.db.
        let expires = entry["expires"]
            .as_f64()
            .or_else(|| entry["expires"].as_str()?.parse().ok())
            .filter(|n| *n > 0.0)
            .map(|n| n as u64);
        return Some(Credential {
            token: clean(entry["access"].as_str())?,
            oauth: true,
            console: clean(entry["metadata"]["server"].as_str()),
            org: clean(entry["metadata"]["orgID"].as_str()),
            expires,
        });
    }
    ["key", "apiKey", "api_key", "token", "accessToken"]
        .iter()
        .find_map(|k| clean(entry[*k].as_str()))
        .map(plain)
}

fn from_database(path: &Path) -> Option<Credential> {
    use rusqlite::{Connection, OpenFlags};
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).ok()?;
    let _ = db.busy_timeout(std::time::Duration::from_millis(500));
    // Newest first, so a rotated sign-in beats the one it replaced. OpenCode has shipped rows with and
    // without an `active` column, and a row omitting it counts as active.
    let rows = |sql: &str| -> Option<Vec<(String, String)>> {
        let mut statement = db.prepare(sql).ok()?;
        let mapped = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .ok()?;
        Some(mapped.flatten().collect())
    };
    let found = rows(
        "SELECT integration_id, value FROM credential WHERE integration_id IN ('opencode-go','opencode') \
         AND COALESCE(active, 1) != 0 ORDER BY time_updated DESC",
    )
    .or_else(|| {
        rows("SELECT integration_id, value FROM credential WHERE integration_id IN ('opencode-go','opencode') ORDER BY rowid DESC")
    })?;
    // Go key first, then the OAuth sign-in, the same order as the file.
    ["opencode-go", "opencode"].iter().find_map(|wanted| {
        found
            .iter()
            .filter(|(id, _)| id == wanted)
            .find_map(|(_, value)| from_value(&serde_json::from_str(value).ok()?))
    })
}

fn from_file(path: &Path) -> Option<Credential> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(1024 * 1024 + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() > 1024 * 1024 {
        return None;
    }
    let root: Value = serde_json::from_slice(&bytes).ok()?;
    if let Some(found) = root.get("opencode-go").and_then(from_value) {
        return Some(found);
    }
    root.get("opencode")
        .filter(|entry| entry["type"].as_str() == Some("oauth"))
        .and_then(from_value)
}

pub fn load(dir: &Path) -> Option<Credential> {
    from_database(&dir.join("opencode.db")).or_else(|| from_file(&dir.join("auth.json")))
}

/// `$XDG_DATA_HOME/opencode`, `~/.local/share/opencode`, then the platform config folder.
pub fn data_dirs(xdg: Option<PathBuf>, home: Option<&Path>, config: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(xdg) = xdg.filter(|p| !p.as_os_str().is_empty()) {
        out.push(xdg.join("opencode"));
    }
    if let Some(home) = home {
        out.push(home.join(".local").join("share").join("opencode"));
    }
    if let Some(config) = config {
        out.push(config.join("opencode"));
    }
    out
}

pub fn load_first(dirs: &[PathBuf]) -> Option<Credential> {
    dirs.iter().find_map(|dir| load(dir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("matra-oc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn database(dir: &Path, with_active: bool, rows: &[(&str, &str, i64)]) {
        let db = rusqlite::Connection::open(dir.join("opencode.db")).unwrap();
        let extra = if with_active { ", active INTEGER" } else { "" };
        db.execute_batch(&format!("CREATE TABLE credential (integration_id TEXT, value TEXT, time_updated INTEGER{extra});")).unwrap();
        for (id, value, time) in rows {
            if with_active {
                db.execute("INSERT INTO credential VALUES (?1,?2,?3,NULL)", rusqlite::params![id, value, time]).unwrap();
            } else {
                db.execute("INSERT INTO credential VALUES (?1,?2,?3)", rusqlite::params![id, value, time]).unwrap();
            }
        }
    }

    #[test]
    fn shapes_and_routes() {
        let key = from_value(&json!({"type":"api","key":"go"})).unwrap();
        assert!(!key.oauth);
        assert_eq!(key.endpoint(), GO_ENDPOINT);
        assert_eq!(from_value(&json!("bare")).unwrap().token, "bare");
        let oauth = from_value(&json!({"type":"oauth","access":"a","expires":"1700000000000",
            "metadata":{"server":"https://console.opencode.ai/x/","orgID":"org"}}))
        .unwrap();
        assert!(oauth.oauth);
        assert_eq!(oauth.endpoint(), OAUTH_ENDPOINT);
        assert_eq!(oauth.org.as_deref(), Some("org"));
        assert_eq!(oauth.expires, Some(1_700_000_000_000));
        assert_eq!(oauth.console_url(), "https://console.opencode.ai/x");
        assert!(from_value(&json!({"type":"oauth"})).is_none());
        assert!(from_value(&json!(5)).is_none());
    }

    #[test]
    fn a_token_never_follows_a_console_address_elsewhere() {
        let mut c = from_value(&json!({"type":"oauth","access":"a","metadata":{"server":"https://evil.example"}})).unwrap();
        assert_eq!(c.console_url(), DEFAULT_CONSOLE);
        c.console = Some("http://opencode.ai/console".into());
        assert_eq!(c.console_url(), DEFAULT_CONSOLE);
        c.console = Some("https://opencode.ai.evil.example".into());
        assert_eq!(c.console_url(), DEFAULT_CONSOLE);
    }

    #[test]
    fn debug_output_cannot_carry_the_token() {
        let c = from_value(&json!("synthetic-secret")).unwrap();
        assert!(!format!("{c:?}").contains("synthetic-secret"));
    }

    #[test]
    fn database_beats_the_dead_key_in_the_file() {
        let dir = temp("order");
        std::fs::write(dir.join("auth.json"), r#"{"opencode-go":{"key":"dead"}}"#).unwrap();
        assert_eq!(load(&dir).unwrap().token, "dead");
        database(
            &dir,
            true,
            &[
                ("opencode-go", r#"{"type":"api","key":"old"}"#, 1),
                ("opencode-go", r#"{"type":"api","key":"rotated"}"#, 2),
                ("openrouter", r#"{"type":"api","key":"other-vendor"}"#, 3),
            ],
        );
        assert_eq!(load(&dir).unwrap().token, "rotated");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_opencodes_own_rows_count() {
        let dir = temp("vendors");
        database(&dir, false, &[("openrouter", r#"{"type":"api","key":"other"}"#, 1)]);
        std::fs::write(dir.join("auth.json"), r#"{"openai":{"key":"other"}}"#).unwrap();
        assert!(load(&dir).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_schema_without_active_still_reads_and_file_oauth_is_a_fallback() {
        let dir = temp("schema");
        database(&dir, false, &[("opencode", r#"{"type":"oauth","access":"tok"}"#, 1)]);
        assert!(load(&dir).unwrap().oauth);
        let other = temp("file-oauth");
        std::fs::write(other.join("auth.json"), r#"{"opencode":{"type":"oauth","access":"f"}}"#).unwrap();
        assert_eq!(load(&other).unwrap().token, "f");
        std::fs::write(other.join("auth.json"), r#"{"opencode":{"type":"api","key":"k"}}"#).unwrap();
        assert!(load(&other).is_none());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn data_dirs_keep_the_existing_search_order() {
        let home = PathBuf::from("/h");
        let config = PathBuf::from("/c");
        let dirs = data_dirs(Some(PathBuf::from("/x")), Some(&home), Some(&config));
        assert_eq!(dirs[0], PathBuf::from("/x/opencode"));
        assert_eq!(dirs[1], home.join(".local").join("share").join("opencode"));
        assert_eq!(dirs[2], config.join("opencode"));
    }
}
