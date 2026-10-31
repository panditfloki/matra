//! The optional OpenCode status bridge. Nothing here runs on its own: `install` and `remove` are reached
//! only from an explicit Settings action, and tests always pass their own directories.
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub const OWNERSHIP_MARKER: &str = "matra-opencode-bridge";
pub const TEMPLATE: &str = include_str!("../../resources/matra-opencode.mjs");
const STEM: &str = "matra";

#[derive(Debug, Clone, Default, Serialize)]
pub struct Activity {
    pub busy: u32,
    pub waiting: u32,
    pub idle: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Status {
    pub installed: bool,
    pub owned: bool,
    pub opencode_found: bool,
    pub location: String,
    pub activity: Activity,
}

/// OpenCode resolves its config directory the XDG way on Windows too.
pub fn plugins_dir(xdg_config: Option<&str>, home: &Path) -> PathBuf {
    let root = xdg_config
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    root.join("opencode").join("plugins")
}

fn is_owned(bytes: &[u8]) -> bool {
    let marker = OWNERSHIP_MARKER.as_bytes();
    bytes.windows(marker.len()).any(|w| w == marker)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let pending = path.with_extension("pending");
    std::fs::write(&pending, bytes)?;
    std::fs::rename(&pending, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&pending);
    })
}

fn fail(_: std::io::Error) -> String {
    "The OpenCode bridge could not be changed. Check that the folder is writable.".into()
}

/// A different file already at the target is kept beside it, outside the plugin loader's extensions.
pub fn install(dir: &Path, template: &str, stamp: u64) -> Result<(), String> {
    if !template.contains(OWNERSHIP_MARKER) {
        return Err("The bundled OpenCode bridge is not recognisable, so nothing was installed.".into());
    }
    std::fs::create_dir_all(dir).map_err(fail)?;
    let target = dir.join(format!("{STEM}.js"));
    if target.exists() {
        let existing = std::fs::read(&target)
            .map_err(|_| "A file is already there and cannot be read, so it was left alone.".to_string())?;
        if existing == template.as_bytes() {
            return Ok(());
        }
        std::fs::write(dir.join(format!("{STEM}.{stamp}.backup")), &existing).map_err(fail)?;
    }
    atomic_write(&target, template.as_bytes()).map_err(fail)
}

/// Removes the bridge only if it is still Mātrā's, then puts back the newest displaced file that was not
/// an older bridge. Returns whether anything was removed. A file the user has since replaced with their
/// own stays exactly as it is.
pub fn remove(dir: &Path) -> Result<bool, String> {
    let target = dir.join(format!("{STEM}.js"));
    let Ok(installed) = std::fs::read(&target) else {
        return Ok(false);
    };
    if !is_owned(&installed) {
        return Ok(false);
    }
    std::fs::remove_file(&target).map_err(fail)?;
    let prefix = format!("{STEM}.");
    let displaced = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|path| {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            name.starts_with(&prefix) && name.ends_with(".backup")
        })
        .filter(|path| std::fs::read(path).is_ok_and(|bytes| !is_owned(&bytes)))
        .max_by_key(|path| {
            let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
            (modified, path.clone())
        });
    if let Some(displaced) = displaced {
        std::fs::rename(displaced, &target).map_err(fail)?;
    }
    Ok(true)
}

/// A count of what the bridge last published, for the settings line. Files untouched for six hours are
/// ignored, because a crashed OpenCode leaves its file behind.
pub fn read_activity(dir: &Path) -> Activity {
    let mut out = Activity::default();
    let fresh = std::time::Duration::from_secs(6 * 3600);
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten().take(64) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let recent = meta.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age < fresh);
        if !recent || meta.len() > 1024 * 1024 {
            continue;
        }
        let Some(root) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
            continue;
        };
        if root["version"].as_i64() != Some(1) {
            continue;
        }
        for session in root["sessions"].as_array().into_iter().flatten() {
            match session["state"].as_str() {
                Some("busy") => out.busy += 1,
                Some("waiting") => out.waiting += 1,
                Some("idle") => out.idle += 1,
                _ => {}
            }
        }
    }
    out
}

/// A home-relative display path, so the UI never holds the user name.
pub fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.to_string_lossy().replace('\\', "/")),
        Err(_) => path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
    }
}

pub fn status(dir: &Path, home: &Path, activity_dir: &Path, opencode_found: bool) -> Status {
    let target = dir.join(format!("{STEM}.js"));
    let bytes = std::fs::read(&target).ok();
    Status {
        installed: bytes.is_some(),
        owned: bytes.as_deref().is_some_and(is_owned),
        opencode_found,
        location: tilde(&target, home),
        activity: read_activity(activity_dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("matra-bridge-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_shipped_resource_is_recognisable_and_targets_windows() {
        assert!(TEMPLATE.contains(OWNERSHIP_MARKER));
        assert!(TEMPLATE.contains("win32"));
        assert!(TEMPLATE.contains("matra-notch"));
        assert!(TEMPLATE.contains("MATRA_ACTIVITY_DIR"));
    }

    #[test]
    fn install_is_idempotent_and_keeps_what_it_displaces() {
        let dir = temp("install");
        let plugins = dir.join("opencode").join("plugins");
        install(&plugins, TEMPLATE, 1).unwrap();
        assert_eq!(std::fs::read_to_string(plugins.join("matra.js")).unwrap(), TEMPLATE);
        install(&plugins, TEMPLATE, 2).unwrap();
        assert_eq!(std::fs::read_dir(&plugins).unwrap().count(), 1);
        std::fs::write(plugins.join("matra.js"), "user's own plugin").unwrap();
        install(&plugins, TEMPLATE, 3).unwrap();
        assert_eq!(std::fs::read_to_string(plugins.join("matra.3.backup")).unwrap(), "user's own plugin");
        assert!(remove(&plugins).unwrap());
        assert_eq!(std::fs::read_to_string(plugins.join("matra.js")).unwrap(), "user's own plugin");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_leaves_a_file_that_is_not_ours() {
        let dir = temp("foreign");
        std::fs::write(dir.join("matra.js"), "not ours").unwrap();
        assert!(!remove(&dir).unwrap());
        assert_eq!(std::fs::read_to_string(dir.join("matra.js")).unwrap(), "not ours");
        assert!(!remove(&dir.join("missing")).unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_does_not_restore_an_older_bridge() {
        let dir = temp("older");
        install(&dir, TEMPLATE, 1).unwrap();
        std::fs::write(dir.join("matra.5.backup"), format!("old {OWNERSHIP_MARKER}")).unwrap();
        assert!(remove(&dir).unwrap());
        assert!(!dir.join("matra.js").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_template_without_the_marker_is_refused() {
        let dir = temp("template");
        assert!(install(&dir, "export default 1", 1).is_err());
        assert!(!dir.join("matra.js").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locations_follow_xdg_and_stay_private_in_the_ui() {
        let home = PathBuf::from("/home/someone");
        assert_eq!(plugins_dir(None, &home), home.join(".config/opencode/plugins"));
        assert_eq!(plugins_dir(Some("relative"), &home), home.join(".config/opencode/plugins"));
        let absolute = std::env::temp_dir().join("x");
        assert_eq!(
            plugins_dir(absolute.to_str(), &home),
            absolute.join("opencode").join("plugins")
        );
        let status = status(&home.join(".config/opencode/plugins"), &home, &home.join("none"), false);
        assert_eq!(status.location, "~/.config/opencode/plugins/matra.js");
        assert!(!status.installed);
        assert_eq!(tilde(&PathBuf::from("/other/place/f.js"), &home), "f.js");
    }

    #[test]
    fn activity_counts_only_recent_known_files() {
        let dir = temp("activity");
        std::fs::write(
            dir.join("a.json"),
            r#"{"version":1,"sessions":[{"state":"busy"},{"state":"waiting"},{"state":"idle"},{"state":"ended"}]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("b.json"), r#"{"version":2,"sessions":[{"state":"busy"}]}"#).unwrap();
        std::fs::write(dir.join("c.json"), "{").unwrap();
        std::fs::write(dir.join("d.txt"), r#"{"version":1,"sessions":[{"state":"busy"}]}"#).unwrap();
        let a = read_activity(&dir);
        assert_eq!((a.busy, a.waiting, a.idle), (1, 1, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
