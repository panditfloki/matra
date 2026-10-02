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
        "{{\"unix_ms\":{millis},\"pid\":{},\"component\":\"{}\",\"version\":\"{}\",\"revision\":\"win185-lifecycle1\",\"phase\":\"{phase}\",\"result\":\"{result}\",\"target_pid\":{}}}\n",
        std::process::id(), env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"),
        target_pid.map(|p| p.to_string()).unwrap_or_else(|| "null".into())
    ))
}

pub fn record(phase: &str, result: &str, target_pid: Option<u32>) -> Result<(), String> {
    let entry = line(phase, result, target_pid)?;
    let root = std::env::var_os("APPDATA").ok_or("Cannot resolve lifecycle log directory.")?;
    let directory = std::path::PathBuf::from(root).join("matra-notch");
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
}
