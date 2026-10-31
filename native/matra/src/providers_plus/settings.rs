//! Durable, non-secret settings beside the app's `config.json`. Writes are atomic and a failure is
//! reported, so the UI never shows a choice that did not stick.
use super::custom::CustomEndpoint;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const SETTINGS_FILE: &str = "provider-settings.json";
const BACKOFF_FILE: &str = "provider-backoff.json";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Every provider is off until the user turns it on.
    #[serde(default)]
    pub enabled: Vec<String>,
    /// Only addresses the user changed from a provider's default.
    #[serde(default)]
    pub endpoints: BTreeMap<String, String>,
    #[serde(default)]
    pub custom: Vec<CustomEndpoint>,
}

fn atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let pending: PathBuf = path.with_extension("json.pending");
    std::fs::write(&pending, text)?;
    std::fs::rename(&pending, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&pending);
    })
}

/// A damaged file is moved aside, not overwritten, so a bad write cannot cost the user their endpoints.
pub fn load(dir: &Path) -> Settings {
    let path = dir.join(SETTINGS_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Settings::default();
    };
    match serde_json::from_str(&text) {
        Ok(settings) => settings,
        Err(_) => {
            let _ = std::fs::rename(&path, path.with_extension("json.damaged"));
            Settings::default()
        }
    }
}

pub fn save(dir: &Path, settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|_| "Could not save provider settings. Nothing changed.".to_string())?;
    atomic(&dir.join(SETTINGS_FILE), &text).map_err(|_| "Could not save provider settings. Nothing changed.".to_string())
}

/// Backoff deadlines outlive the process, so a restart cannot be used to poll into a rate limit.
pub fn load_backoff(dir: &Path) -> BTreeMap<String, u64> {
    std::fs::read_to_string(dir.join(BACKOFF_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_backoff(dir: &Path, deadlines: &BTreeMap<String, u64>) {
    if let Ok(text) = serde_json::to_string(deadlines) {
        let _ = atomic(&dir.join(BACKOFF_FILE), &text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("matra-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn fresh_is_all_off_and_round_trips() {
        let dir = temp("roundtrip");
        assert_eq!(load(&dir), Settings::default());
        let settings = Settings {
            enabled: vec!["omlx".into()],
            ..Default::default()
        };
        save(&dir, &settings).unwrap();
        assert_eq!(load(&dir), settings);
        assert!(!dir.join("provider-settings.json.pending").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_file_is_set_aside_not_lost() {
        let dir = temp("damaged");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("provider-settings.json"), "{broken").unwrap();
        assert_eq!(load(&dir), Settings::default());
        assert_eq!(std::fs::read_to_string(dir.join("provider-settings.json.damaged")).unwrap(), "{broken");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deadlines_persist() {
        let dir = temp("backoff");
        let mut deadlines = BTreeMap::new();
        deadlines.insert("deepseek".to_string(), 123u64);
        save_backoff(&dir, &deadlines);
        assert_eq!(load_backoff(&dir), deadlines);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
