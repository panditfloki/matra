//! matra-hook: the minimal client Claude Code's hooks call.
//! Duties: 1) report the event plus stdin JSON to the main app; 2) launch the main app if it is not running.
//! Iron rule: never block Claude Code — ~2 s total budget, and every failure exits 0 silently.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

mod setup;
#[path = "../../release_channel.rs"]
mod release_channel;
#[path = "../../setup_audit.rs"]
mod setup_audit;

const DEFAULT_PORT: u16 = release_channel::EVENT_PORT;
const MAX_STDIN: u64 = 256 * 1024;

fn main() {
    let event = std::env::args().nth(1).unwrap_or_else(|| "ping".into());
    if std::env::var_os("MATRA_PREVIEW").is_some_and(|v| v == "1") {
        // Preview must never inspect stdin, send a hook event, start a real application or prepare
        // an installation. Hook callers still get their fail-open zero exit for ordinary events.
        if matches!(event.as_str(), "--prepare-install" | "--prepare-preview-install") {
            eprintln!("Installer preparation is disabled in the isolated preview.");
            std::process::exit(1);
        }
        return;
    }
    if release_channel::REVIEW_BUILD && event == "--prepare-install" {
        eprintln!("The development helper cannot prepare a production installation.");
        std::process::exit(1);
    }
    if data_dir().is_none() { return; }
    // Installer-only path. Never read Claude's stdin or start the GUI here.
    if matches!(event.as_str(), "--prepare-install" | "--prepare-preview-install") {
        let preview_install = event == "--prepare-preview-install";
        let result = setup_audit::record("prepare", "started", None).and_then(|_| std::env::args().nth(2).ok_or_else(|| "Missing installation path".to_string()))
            .and_then(|path| if preview_install { setup::prepare_preview(&path) } else { setup::prepare(&path) });
        match result {
            Ok(()) => {
                if let Err(error) = setup_audit::record("prepare", "ready", None) {
                    eprintln!("{error}"); std::process::exit(1);
                }
                println!("Matra installation directory is ready.");
            }
            Err(error) => {
                let _ = setup_audit::record("prepare", "failed", None);
                eprintln!("{error}"); std::process::exit(1);
            }
        }
        return;
    }

    // The hook's stdin is the JSON Claude Code provides (session_id / cwd / prompt / message…)
    let mut body = String::new();
    let _ = std::io::stdin().take(MAX_STDIN).read_to_string(&mut body);

    let port = read_port();
    let ppid = parent_pid();

    if send(port, &event, ppid, &body).is_ok() {
        return;
    }
    // Main app not running: launch it detached, then retry briefly
    spawn_main();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(100));
        if send(port, &event, ppid, &body).is_ok() {
            return;
        }
    }
    // Give up quietly — never affect Claude Code
}

/// Same profile root as the shared app, without a dependency or a change to HOME.
fn data_dir() -> Option<std::path::PathBuf> {
    use std::path::{Component, PathBuf};
    if let Some(raw) = std::env::var_os("MATRA_DATA_DIR") {
        let path = PathBuf::from(raw);
        return (path.is_absolute() && !path.components().any(|c| matches!(c, Component::ParentDir))).then_some(path);
    }
    #[cfg(windows)]
    let root = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"));
    #[cfg(not(any(windows, target_os = "macos")))]
    let root = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    root.filter(|p| p.is_absolute()).map(|p| p.join(release_channel::DATA_NAMESPACE))
}

/// Pulls a numeric port out of the app-owned config (hand-rolled scan, no dependency).
fn read_port() -> u16 {
    if release_channel::REVIEW_BUILD { return DEFAULT_PORT; }
    let Some(path) = data_dir().map(|p| p.join("config.json")) else { return DEFAULT_PORT; };
    let Ok(txt) = std::fs::read_to_string(path) else {
        return DEFAULT_PORT;
    };
    parse_port(&txt).unwrap_or(DEFAULT_PORT)
}

fn parse_port(txt: &str) -> Option<u16> {
    let tail = txt.split_once("\"port\"")?.1.trim_start().strip_prefix(':')?.trim_start();
    let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
    let value: u16 = digits.parse().ok()?;
    (value != 0).then_some(value)
}

fn send(port: u16, event: &str, ppid: u32, body: &str) -> std::io::Result<()> {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(300))?;
    s.set_write_timeout(Some(Duration::from_millis(700)))?;
    s.set_read_timeout(Some(Duration::from_millis(700)))?;
    let req = format!(
        "POST /event?e={}&ppid={} HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Matra-Channel: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        event,
        ppid,
        release_channel::BUNDLE_ID,
        body.len(),
        body
    );
    s.write_all(req.as_bytes())?;
    let mut buf = [0u8; 64];
    let _ = s.read(&mut buf); // wait for a response fragment to confirm delivery; failure does not matter
    Ok(())
}

/// Launches the main app detached: no inherited handles, no window, never waits
fn spawn_main() {
    let Ok(me) = std::env::current_exe() else { return };
    let Some(dir) = me.parent() else { return };
    let exe = dir.join(if cfg!(windows) { "matra.exe" } else { "matra-development020" });
    if !exe.exists() {
        return;
    }
    let mut cmd = std::process::Command::new(exe);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let _ = cmd.spawn();
}

/// Parent process PID (≈ the Claude Code CLI process) via NtQueryInformationProcess, no dependency
#[cfg(windows)]
fn parent_pid() -> u32 {
    #[repr(C)]
    struct Pbi {
        exit_status: isize,
        peb: usize,
        affinity_mask: usize,
        base_priority: isize,
        unique_process_id: usize,
        inherited_from_unique_process_id: usize,
    }
    extern "system" {
        fn NtQueryInformationProcess(
            handle: isize,
            class: u32,
            info: *mut Pbi,
            len: u32,
            ret_len: *mut u32,
        ) -> i32;
    }
    unsafe {
        let mut pbi = std::mem::zeroed::<Pbi>();
        let mut ret = 0u32;
        // -1 = GetCurrentProcess()
        if NtQueryInformationProcess(
            -1,
            0,
            &mut pbi,
            std::mem::size_of::<Pbi>() as u32,
            &mut ret,
        ) == 0
        {
            return pbi.inherited_from_unique_process_id as u32;
        }
    }
    0
}

#[cfg(not(windows))]
fn parent_pid() -> u32 {
    extern "C" { fn getppid() -> i32; }
    unsafe { getppid().max(0) as u32 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn configured_port_requires_a_number_and_valid_tcp_range() {
        assert_eq!(super::parse_port(r#"{"port":48680,"darkMode":true}"#), Some(48680));
        for invalid in [r#"{"port":"48680"}"#, r#"{"port":-1}"#, r#"{"port":0}"#, r#"{"port":65536}"#] {
            assert_eq!(super::parse_port(invalid), None);
        }
    }
    #[cfg(unix)]
    #[test]
    fn parent_process_is_read_without_shelling_out() { assert!(super::parent_pid() > 0); }
}
