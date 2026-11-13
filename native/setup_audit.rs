//! Local lifecycle evidence. Deliberately excludes paths, commands and account data.
use std::io::Write;

fn line(phase: &str, result: &str, target_pid: Option<u32>) -> Result<String, String> {
    // Reject arbitrary error text, paths and control characters at the log boundary.
    for value in [phase, result] {
        if value.is_empty()
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_".contains(&c))
        {
            return Err("Invalid lifecycle diagnostic label.".into());
        }
    }
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "Cannot timestamp lifecycle diagnostics.")?
        .as_millis();
    Ok(format!(
        "{{\"unix_ms\":{millis},\"pid\":{},\"component\":\"{}\",\"version\":\"{}\",\"revision\":\"development020-lifecycle1\",\"phase\":\"{phase}\",\"result\":\"{result}\",\"target_pid\":{}}}\n",
        std::process::id(), env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"),
        target_pid.map(|p| p.to_string()).unwrap_or_else(|| "null".into())
    ))
}

fn absolute_directory(raw: &std::ffi::OsStr) -> Result<std::path::PathBuf, String> {
    use std::path::{Component, PathBuf};
    let path = PathBuf::from(raw);
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("Lifecycle data directory must be absolute without parent components.".into());
    }
    let mut ancestor = path.as_path();
    let mut missing = Vec::new();
    while !ancestor.exists() {
        missing.push(ancestor.file_name().ok_or("Cannot resolve lifecycle data directory.")?.to_os_string());
        ancestor = ancestor.parent().ok_or("Cannot resolve lifecycle data directory.")?;
    }
    let mut resolved = ancestor.canonicalize().map_err(|_| "Cannot resolve lifecycle data directory.")?;
    if !resolved.is_dir() { return Err("Lifecycle data directory is not a directory.".into()); }
    for part in missing.into_iter().rev() { resolved.push(part); }
    Ok(resolved)
}

fn directory() -> Result<std::path::PathBuf, String> {
    use std::path::PathBuf;
    if let Some(raw) = std::env::var_os("MATRA_DATA_DIR") {
        return absolute_directory(&raw);
    }
    if std::env::var_os("MATRA_PREVIEW").is_some_and(|v| v == "1") {
        return Err("Isolated preview requires MATRA_DATA_DIR for lifecycle diagnostics.".into());
    }
    #[cfg(test)]
    {
        let root = std::env::var_os("MATRA_TEST_WORKSPACE").map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".test-runtime"));
        return Ok(root.join("runtime").join(crate::release_channel::DATA_NAMESPACE));
    }
    #[cfg(not(test))]
    {
        let namespace = crate::release_channel::DATA_NAMESPACE;
        #[cfg(windows)]
        let root = std::env::var_os("APPDATA").map(PathBuf::from);
        #[cfg(target_os = "macos")]
        let root = std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"));
        #[cfg(not(any(windows, target_os = "macos")))]
        let root = std::env::var_os("XDG_CONFIG_HOME").filter(|p| PathBuf::from(p).is_absolute())
            .map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")));
        let root = root.ok_or("Cannot resolve lifecycle log directory.")?;
        absolute_directory(root.join(namespace).as_os_str())
    }
}

pub fn record(phase: &str, result: &str, target_pid: Option<u32>) -> Result<(), String> {
    let entry = line(phase, result, target_pid)?;
    let directory = directory()?;
    std::fs::create_dir_all(&directory).map_err(|_| "Cannot create lifecycle log directory.")?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("setup-events.jsonl"))
        .map_err(|_| "Cannot open lifecycle log.")?;
    file.write_all(entry.as_bytes())
        .and_then(|_| file.flush())
        .map_err(|_| "Cannot write lifecycle log.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_records_are_timestamped_and_reject_sensitive_free_text() {
        let row = line("prepare", "close-requested", Some(42)).unwrap();
        assert!(row.contains("\"unix_ms\":"));
        assert!(row.contains("\"target_pid\":42"));
        assert!(row.ends_with('\n'));
        for invalid in [
            "",
            "C:\\Users\\private",
            "error\nforged",
            "token=secret",
            "path with spaces",
        ] {
            assert!(line("prepare", invalid, None).is_err());
        }
    }

    #[test]
    fn lifecycle_override_rejects_relative_and_parent_paths() {
        assert!(absolute_directory(std::ffi::OsStr::new("relative/profile")).is_err());
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert!(absolute_directory(root.join("../escape").as_os_str()).is_err());
        assert!(absolute_directory(root.join(".test-runtime/audit").as_os_str()).unwrap().is_absolute());
    }
}
