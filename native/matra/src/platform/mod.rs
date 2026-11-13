//! The shared application's small OS boundary. No shell interpolation or profile mutation.
pub mod carry;
use serde::Serialize;
use std::{ffi::OsStr, process::{Command, Stdio}, time::{Duration, Instant}};

pub fn preview() -> bool { std::env::var_os("MATRA_PREVIEW").is_some_and(|v| v == "1") }

pub fn review_build() -> bool { crate::release_channel::REVIEW_BUILD }

pub fn require_integration(action: &str) -> Result<(), String> { require_live(action) }
pub fn require_automatic_integration(action: &str) -> Result<(), String> {
    require_live(action)?;
    if review_build() { Err(format!("{action} is disabled for Development 0.2.0. Use an explicit Settings action for development hooks or login startup.")) }
    else { Ok(()) }
}

pub fn require_live(action: &str) -> Result<(), String> {
    if preview() { Err(format!("{action} is disabled in the isolated preview. Quit preview and open the installed app to use it.")) }
    else { Ok(()) }
}

pub fn linux_x11_session() -> bool {
    std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty())
        && !std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
        && std::env::var_os("WAYLAND_DISPLAY").is_none()
}

pub fn global_pointer_available() -> bool { cfg!(windows) || (cfg!(target_os = "linux") && linux_x11_session()) }

pub fn updates_available() -> bool {
    !preview() && cfg!(windows) && !review_build()
}

#[derive(Serialize)]
pub struct Capabilities {
    autostart: bool,
    notifications: bool,
    secure_storage: bool,
    native_materials: bool,
    updates: bool,
    hooks: bool,
    notch_drag: bool,
    global_pointer: bool,
    absolute_window_placement: bool,
    terminal_focus: bool,
}

#[derive(Serialize)]
pub struct PlatformInfo {
    os: &'static str,
    label: &'static str,
    preview: bool,
    capabilities: Capabilities,
    limitations: Vec<String>,
}

#[tauri::command]
pub fn get_platform_info() -> PlatformInfo {
    let (os, label) = if cfg!(windows) { ("windows", "Windows") }
        else if cfg!(target_os = "macos") { ("macos", "macOS") }
        else { ("linux", "Linux") };
    let mut limitations = Vec::new();
    let global = global_pointer_available();
    if preview() {
        limitations.push("ISOLATED PREVIEW: all displayed quotas are synthetic sample data. Provider access, credentials, hooks, login startup, notifications, network services and updates are disabled.".into());
    }
    if !cfg!(windows) {
        limitations.push("Terminal jump-back, foreground-session acknowledgement and desktop network-activity detection are not available in this shared build.".into());
        limitations.push("Encrypted Cursor editor credentials are not yet supported here. No alternate account is selected when decryption is unavailable.".into());
        limitations.push("Automatic installer updates are Windows-only in this shared build. Use a separately verified package for this operating system.".into());
    }
    if cfg!(target_os = "linux") {
        if !linux_x11_session() {
            limitations.push("Wayland restricts global pointer input and absolute window positioning. Perimeter carry, monitor/edge positioning and desktop hover detection require an X11 login session; standard dashboard and settings remain available.".into());
        }
        limitations.push("Provider keys use the desktop Secret Service. Locked or unavailable storage remains unavailable; background reads never request unlock.".into());
    }
    if !cfg!(windows) {
        limitations.push("The Antigravity CLI pseudoterminal adapter is Windows-only; supported local bridge/cache sources remain separate. Claude sign-in must be completed in your own terminal.".into());
    }
    if review_build() {
        limitations.push("Development 0.2.0 uses separate application data and key storage. Automatic account renewal, hook migration and installer updates are disabled. Explicit hook/startup settings use a separate development identity.".into());
    }
    PlatformInfo { os, label, preview: preview(), capabilities: Capabilities {
        autostart: !preview() && cfg!(any(windows, target_os = "macos", target_os = "linux")),
        notifications: !preview() && crate::alerts::notifications_available(),
        secure_storage: !preview() && crate::providers_plus::vault::AVAILABLE,
        native_materials: crate::settings_window::has_native_materials(),
        updates: updates_available(),
        hooks: !preview(),
        notch_drag: global,
        global_pointer: global,
        absolute_window_placement: global,
        terminal_focus: cfg!(windows),
    }, limitations }
}

/// A subprocess may submit a request to the OS; its successful exit does not prove a window or
/// notification was actually visible. No output containing provider data is returned or logged.
pub fn submit(mut command: Command, input: Option<&[u8]>) -> Result<(), String> {
    use std::io::Write;
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = command.spawn().map_err(|_| "The operating-system helper could not be started.".to_string())?;
    if let Some(bytes) = input {
        if child.stdin.take().is_none_or(|mut stdin| stdin.write_all(bytes).is_err()) {
            let _ = child.kill(); let _ = child.wait();
            return Err("The operating-system helper did not accept the request.".into());
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err("The operating system rejected the request. Check its permissions and default applications.".into()),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill(); let _ = child.wait();
                return Err("The operating-system helper did not finish. Check its permissions and try again.".into());
            }
        }
    }
}

pub fn open(target: &OsStr) -> Result<(), String> {
    require_live("Opening an external application")?;
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("explorer.exe");
        command.creation_flags(0x0800_0000);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = { let mut command = Command::new("/usr/bin/open"); command.arg("--"); command };
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    command.arg(target);
    // Opening a browser/file manager is an explicit action. Spawn errors are actionable; the
    // helper's exit alone cannot establish that its UI became visible.
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    command.spawn().map(|_| ()).map_err(|_| "Could not open the default browser or file manager. Check the desktop's default applications.".into())
}

pub fn executable(path: &std::path::Path) -> bool {
    if !path.is_file() { return false; }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    { true }
}

#[cfg(test)]
mod tests {
    #[test]
    fn review_channel_keeps_ordinary_launch_integrations_disabled() {
        assert!(super::review_build());
        assert_eq!(crate::release_channel::DATA_NAMESPACE, "matra-development-0.2.0");
        assert!(!super::updates_available());
        assert!(super::require_automatic_integration("Review fixture").is_err());
        let info = serde_json::to_value(super::get_platform_info()).unwrap();
        for key in ["updates"] {
            assert_eq!(info["capabilities"][key], false, "{key}");
        }
    }

    #[test]
    fn platform_contract_has_only_supported_identifiers_and_capability_names() {
        let info = serde_json::to_value(super::get_platform_info()).unwrap();
        assert!(["windows", "macos", "linux"].contains(&info["os"].as_str().unwrap()));
        assert!(info["label"].is_string());
        for key in ["autostart", "notifications", "secure_storage", "native_materials", "updates", "hooks", "notch_drag"] {
            assert!(info["capabilities"][key].is_boolean());
        }
        assert!(info["limitations"].is_array());
    }
}
