//! Start at sign-in: an HKCU\...\Run registry value (per user, no administrator needed).
//! The command carries --silent: a duplicate login launch must not open Settings.
//! Implemented with reg.exe, so no new dependency.

use std::process::Command;

const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const NAME: &str = "MatraNotch";
const APPROVED_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

fn command_for(exe: &std::path::Path) -> String {
    format!("\"{}\" --silent", exe.display())
}
fn matches_command(output: &str, expected: &str) -> bool {
    output.lines().filter_map(|line| line.split_once("REG_SZ"))
        .any(|(_, value)| value.trim().eq_ignore_ascii_case(expected))
}
fn blocked(output: &str) -> bool {
    output.lines().filter_map(|line| line.split_once("REG_BINARY"))
        .any(|(_, value)| value.trim().starts_with("03") || value.trim().starts_with("07"))
}

fn reg(args: &[&str]) -> Option<(bool, String)> {
    let mut c = Command::new("reg");
    c.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    c.output().ok().map(|o| {
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        (o.status.success(), text)
    })
}

pub fn is_enabled() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    if reg(&["query", APPROVED_KEY, "/v", NAME]).is_some_and(|(ok, out)| ok && blocked(&out)) {
        return false;
    }
    reg(&["query", RUN_KEY, "/v", NAME])
        .map(|(ok, out)| ok && matches_command(&out, &command_for(&exe)))
        .unwrap_or(false)
}

pub fn enable() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let val = command_for(&exe);
    if let Some((true, out)) = reg(&["query", APPROVED_KEY, "/v", NAME]) {
        if blocked(&out) {
            match reg(&["add", APPROVED_KEY, "/v", NAME, "/t", "REG_BINARY", "/d", "020000000000000000000000", "/f"]) {
                Some((true, _)) => {},
                _ => return Err("Windows disabled startup. Enable Matra in Windows Settings > Apps > Startup.".into()),
            }
        }
    }
    match reg(&["add", RUN_KEY, "/v", NAME, "/t", "REG_SZ", "/d", &val, "/f"]) {
        Some((true, _)) if is_enabled() => Ok("Start at login enabled for this installed Matra.".into()),
        Some((true, _)) => Err("Startup registration could not be verified.".into()),
        Some((false, out)) => Err(out),
        None => Err("reg.exe failed to run".into()),
    }
}

pub fn disable() -> Result<String, String> {
    if reg(&["query", RUN_KEY, "/v", NAME]).is_some_and(|(ok, _)| !ok) {
        return Ok("Start at login is off.".into());
    }
    match reg(&["delete", RUN_KEY, "/v", NAME, "/f"]) {
        Some((true, _)) => Ok("start at sign-in disabled".into()),
        Some((false, out)) => {
            if out.to_lowercase().contains("unable to find") || out.contains("找不到") { // reg.exe answers in the OS language; "找不到" is the Chinese "unable to find"
                Ok("start at sign-in was not enabled".into())
            } else {
                Err(out)
            }
        }
        None => Err("reg.exe failed to run".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_target_and_windows_disable_state() {
        let command = command_for(std::path::Path::new(r"C:\Program Files\Matra\matra.exe"));
        assert_eq!(command, "\"C:\\Program Files\\Matra\\matra.exe\" --silent");
        assert!(matches_command(&format!("MatraNotch    REG_SZ    {command}"), &command));
        assert!(!matches_command("MatraNotch REG_SZ C:\\old\\matra.exe", &command));
        assert!(blocked("MatraNotch REG_BINARY 030000000000000000000000"));
        assert!(!blocked("MatraNotch REG_BINARY 020000000000000000000000"));
    }
}
