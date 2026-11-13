//! Per-user startup adapters. Changes happen only after explicit opt-in.
#[cfg(windows)]
mod implementation {
//! Per-user startup. Native UTF-16 registry access, independent of OS language.
use std::path::Path;
use windows::core::HSTRING;
use windows::Win32::System::Registry::*;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "MatraDevelopment020";

fn command_for(exe: &Path) -> String {
    format!("\"{}\" --silent", exe.display())
}

fn read(key: &str, flags: REG_ROUTINE_FLAGS) -> Result<Option<Vec<u8>>, String> {
    let mut size = 0;
    let key = HSTRING::from(key);
    let name = HSTRING::from(NAME);
    unsafe {
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            &name,
            flags,
            None,
            None,
            Some(&mut size),
        );
        if matches!(status.0, 2 | 3) {
            return Ok(None);
        }
        status.ok().map_err(|e| e.to_string())?;
        if size > 65536 {
            return Err("Startup value is too large; left untouched.".into());
        }
        let mut bytes = vec![0; size as usize];
        RegGetValueW(
            HKEY_CURRENT_USER,
            &key,
            &name,
            flags,
            None,
            Some(bytes.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .map_err(|e| e.to_string())?;
        bytes.truncate(size as usize);
        Ok(Some(bytes))
    }
}

fn decode_string(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() % 2 != 0 {
        return Err("Invalid startup registry string".into());
    }
    let mut wide: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    while wide.last() == Some(&0) {
        wide.pop();
    }
    String::from_utf16(&wide).map_err(|e| e.to_string())
}

fn registered() -> Result<Option<String>, String> {
    read(RUN_KEY, RRF_RT_REG_SZ)?
        .map(|b| decode_string(&b))
        .transpose()
}

fn blocked() -> Result<bool, String> {
    match read(APPROVED_KEY, RRF_RT_REG_BINARY)? {
        None => Ok(false),
        Some(bytes) => approval_blocked(&bytes),
    }
}

fn approval_blocked(bytes: &[u8]) -> Result<bool, String> {
    let word: [u8; 4] = bytes
        .get(..4)
        .ok_or("Invalid Windows startup approval; left untouched.")?
        .try_into()
        .map_err(|_| "Invalid Windows startup approval.")?;
    match u32::from_le_bytes(word) {
        2 | 6 => Ok(false),
        3 | 7 => Ok(true),
        _ => Err("Unknown Windows startup approval; left untouched.".into()),
    }
}

fn write_command(command: &str) -> Result<(), String> {
    let mut key = HKEY::default();
    unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN_KEY),
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
        .ok()
        .map_err(|e| e.to_string())?;
        let bytes: Vec<u8> = command
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        let result = RegSetValueExW(key, &HSTRING::from(NAME), 0, REG_SZ, Some(&bytes)).ok();
        let _ = RegCloseKey(key);
        result.map_err(|e| e.to_string())
    }
}

pub fn status() -> Result<bool, String> {
    crate::platform::require_integration("Login startup access")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(!blocked()? && registered()?.is_some_and(|v| v.eq_ignore_ascii_case(&command_for(&exe))))
}

pub fn enable() -> Result<String, String> {
    crate::platform::require_integration("Login startup access")?;
    if blocked()? {
        return Err("Windows disabled startup. Enable Matra in Windows Settings > Apps > Startup, then try again.".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let before = registered()?;
    if let Some(existing) = &before {
        if !crate::hooks_install::owned_executable(&existing, &exe) {
            return Err(
                "Another installation owns the Matra startup entry; it was left untouched.".into(),
            );
        }
    }
    let desired = command_for(&exe);
    if !before
        .as_ref()
        .is_some_and(|v| v.eq_ignore_ascii_case(&desired))
    {
        write_command(&desired)?;
    }
    if !status()? {
        return Err("Startup registration could not be verified.".into());
    }
    Ok("Start at login enabled for this installed Matra.".into())
}

pub struct Migration {
    before: Option<String>,
    desired: Option<String>,
}

fn migration_for(
    before: Option<String>,
    desired: String,
    owns: impl Fn(&str) -> bool,
) -> Migration {
    let needs_write = before
        .as_ref()
        .is_some_and(|v| owns(v) && !v.eq_ignore_ascii_case(&desired));
    Migration {
        before,
        desired: needs_write.then_some(desired),
    }
}

/// Read/validate before any integration is mutated. An absent opt-in stays absent.
pub fn prepare_migration() -> Result<Migration, String> {
    crate::platform::require_integration("Login startup access")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(migration_for(registered()?, command_for(&exe), |v| {
        crate::hooks_install::owned_executable(v, &exe)
    }))
}

impl Migration {
    pub fn validate(&self) -> Result<(), String> {
        if registered()? != self.before {
            return Err(
                "Startup registration changed during setup; left untouched. Retry setup.".into(),
            );
        }
        Ok(())
    }
    pub fn apply(self) -> Result<&'static str, String> {
        self.validate()?;
        if let Some(command) = self.desired {
            write_command(&command)?;
            if registered()?.as_deref() != Some(command.as_str()) {
                return Err("Migrated startup command could not be verified.".into());
            }
            Ok("retargeted")
        } else {
            Ok("unchanged")
        }
    }
}

pub fn disable() -> Result<String, String> {
    crate::platform::require_integration("Login startup access")?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let Some(existing) = registered()? else {
        return Ok("Start at login is off.".into());
    };
    if !crate::hooks_install::owned_executable(&existing, &exe) {
        return Ok("Startup belongs to another installation and was preserved.".into());
    }
    let mut key = HKEY::default();
    unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN_KEY),
            0,
            KEY_SET_VALUE,
            &mut key,
        )
        .ok()
        .map_err(|e| e.to_string())?;
        let status = RegDeleteValueW(key, &HSTRING::from(NAME));
        let _ = RegCloseKey(key);
        if status.0 != 2 {
            status.ok().map_err(|e| e.to_string())?;
        }
    }
    Ok("Start at login is off.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_registry_command_round_trips() {
        let command = command_for(Path::new(r"C:\Users\पंडित\Mātrā\matra.exe"));
        let bytes: Vec<u8> = command
            .encode_utf16()
            .chain(Some(0))
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(decode_string(&bytes).unwrap(), command);
        assert!(decode_string(&[0]).is_err());
        assert!(command.ends_with("\" --silent"));
    }
    #[test]
    fn migration_writes_only_a_changed_owned_opt_in() {
        let command = command_for(Path::new(r"C:\Apps\Mātrā\matra.exe"));
        let own = |s: &str| s.contains("Apps") || s.contains("legacy");
        for before in [
            None,
            Some(command.clone()),
            Some(r#""D:\foreign\matra.exe" --silent"#.into()),
        ] {
            assert!(migration_for(before, command.clone(), own)
                .desired
                .is_none());
        }
        let old = Some(r#""C:\legacy\matra.exe" --silent"#.into());
        let plan = migration_for(old.clone(), command.clone(), own);
        assert_eq!(plan.before, old);
        assert_eq!(plan.desired, Some(command));
    }
    #[test]
    fn windows_disabled_and_unknown_approval_are_not_enabled() {
        assert_eq!(approval_blocked(&3u32.to_le_bytes()), Ok(true));
        assert_eq!(approval_blocked(&7u32.to_le_bytes()), Ok(true));
        assert_eq!(approval_blocked(&2u32.to_le_bytes()), Ok(false));
        assert!(approval_blocked(&[2]).is_err());
        assert!(approval_blocked(&99u32.to_le_bytes()).is_err());
    }
}

}
#[cfg(windows)]
pub use implementation::*;

#[cfg(not(windows))]
mod posix {
    use std::path::{Path, PathBuf};
    const OWNER: &str = "Managed by Matra Development 0.2.0 autostart v1";
    const LABEL: &str = "com.dydxfx.matra.development020";

    fn path() -> Result<PathBuf, String> {
        #[cfg(target_os = "macos")]
        return dirs::home_dir().map(|h| h.join("Library/LaunchAgents").join(format!("{LABEL}.plist")))
            .ok_or_else(|| "Cannot locate your LaunchAgents directory.".into());
        #[cfg(not(target_os = "macos"))]
        return dirs::config_dir().map(|d| d.join("autostart/com.dydxfx.matra.development020.desktop"))
            .ok_or_else(|| "Cannot locate your XDG configuration directory.".into());
    }

    fn xml(value: &str) -> String {
        value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
            .replace('"', "&quot;").replace('\'', "&apos;")
    }

    fn desktop_arg(value: &str) -> Result<String, String> {
        if value.chars().any(char::is_control) { return Err("The application path contains unsupported control characters.".into()); }
        // The Desktop Entry Exec grammar is not a shell: quoted reserved characters are escaped,
        // and literal percent must be doubled to avoid interpretation as a field code.
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"").replace('`', "\\`")
            .replace('$', "\\$").replace('%', "%%");
        Ok(format!("\"{escaped}\""))
    }

    fn desired(exe: &Path) -> Result<String, String> {
        let exe = exe.to_str().ok_or("The application path must be valid UTF-8 for login startup.")?;
        #[cfg(target_os = "macos")]
        return Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{LABEL}</string><key>ProgramArguments</key><array><string>{}</string><string>--silent</string></array><key>RunAtLoad</key><true/><key>ProcessType</key><string>Interactive</string><key>Comment</key><string>{OWNER}</string></dict></plist>\n", xml(exe)));
        #[cfg(not(target_os = "macos"))]
        return Ok(format!("[Desktop Entry]\nType=Application\nVersion=1.0\nName=Matra Development 0.2.0\nComment={OWNER}\nExec={} --silent\nTerminal=false\nX-GNOME-Autostart-enabled=true\n", desktop_arg(exe)?));
    }

    fn read(path: &Path) -> Result<Option<String>, String> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() =>
                return Err("Login startup is not a regular owned file; it was left untouched.".into()),
            Ok(_) => {},
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("Cannot inspect login startup. Check directory permissions.".into()),
        }
        std::fs::read_to_string(path).map(Some).map_err(|_| "Cannot read login startup; it was left untouched.".into())
    }

    fn owned(value: &str) -> bool { value.contains(OWNER) }

    fn write(path: &Path, value: &str, before: Option<&str>) -> Result<(), String> {
        if read(path)?.as_deref() != before { return Err("Login startup changed. Retry without overwriting another installation.".into()); }
        let parent = path.parent().ok_or("Cannot resolve login startup directory.")?;
        std::fs::create_dir_all(parent).map_err(|_| "Cannot create login startup directory.".to_string())?;
        let parent = parent.canonicalize().map_err(|_| "Cannot resolve login startup directory.".to_string())?;
        let path = parent.join(path.file_name().ok_or("Invalid login startup path.")?);
        let pending = path.with_extension(format!("matra-pending-{}", std::process::id()));
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&pending)
            .map_err(|_| "Cannot prepare login startup. Existing settings were preserved.".to_string())?;
        let result = file.write_all(value.as_bytes()).and_then(|_| file.sync_all());
        drop(file);
        if result.is_err() { let _ = std::fs::remove_file(&pending); return Err("Cannot write login startup.".into()); }
        if read(&path)?.as_deref() != before { let _ = std::fs::remove_file(&pending); return Err("Login startup changed; existing settings were preserved.".into()); }
        std::fs::rename(&pending, &path).map_err(|_| "Cannot save login startup.".to_string())?;
        if read(&path)?.as_deref() != Some(value) { return Err("Saved login startup could not be verified.".into()); }
        Ok(())
    }

    pub fn status() -> Result<bool, String> {
        crate::platform::require_integration("Reading login startup")?;
        let exe = std::env::current_exe().map_err(|_| "Cannot locate this application.".to_string())?;
        Ok(read(&path()?)?.as_deref() == Some(desired(&exe)?.as_str()))
    }

    pub fn enable() -> Result<String, String> {
        crate::platform::require_integration("Changing login startup")?;
        let path = path()?;
        let before = read(&path)?;
        let desired = desired(&std::env::current_exe().map_err(|_| "Cannot locate this application.".to_string())?)?;
        if before.as_ref().is_some_and(|s| !owned(s)) {
            return Err("Another application owns this login startup file; it was left untouched.".into());
        }
        if before.as_deref() != Some(&desired) { write(&path, &desired, before.as_deref())?; }
        Ok("Start at login registered for the next desktop login. Operating-system login-item policy may still disable it.".into())
    }

    pub fn disable() -> Result<String, String> {
        crate::platform::require_integration("Changing login startup")?;
        let path = path()?;
        let Some(before) = read(&path)? else { return Ok("Start at login is off.".into()); };
        if !owned(&before) { return Err("Another application owns this login startup file; it was preserved.".into()); }
        std::fs::remove_file(path).map_err(|_| "Cannot remove login startup. Check directory permissions.".to_string())?;
        Ok("Start at login is off.".into())
    }

    pub struct Migration;
    impl Migration {
        pub fn validate(&self) -> Result<(), String> { crate::platform::require_integration("Migrating login startup") }
        pub fn apply(self) -> Result<&'static str, String> { self.validate()?; Ok("unchanged") }
    }
    pub fn prepare_migration() -> Result<Migration, String> {
        crate::platform::require_integration("Migrating login startup")?;
        // Windows installer migration has no authority over Swift login items or another Linux
        // installation. A fresh opt-in uses this app's own shared identity.
        Ok(Migration)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn startup_paths_are_data_and_foreign_files_have_no_ownership() {
            let output = desired(Path::new("/fixture/O'Brien $bin/Mātrā%20")).unwrap();
            assert!(output.contains(OWNER));
            assert!(!owned("[Desktop Entry]\nExec=another-app"));
            assert_eq!(xml("a&<b>"), "a&amp;&lt;b&gt;");
            assert_eq!(desktop_arg("/a/$b\"%c").unwrap(), "\"/a/\\$b\\\"%%c\"");
            assert!(desktop_arg("/a\nb").is_err());
        }
    }
}
#[cfg(not(windows))]
pub use posix::*;
