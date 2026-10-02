//! Per-user startup. Native UTF-16 registry access, independent of OS language.
use std::path::Path;
use windows::core::HSTRING;
use windows::Win32::System::Registry::*;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const NAME: &str = "MatraNotch";

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
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(!blocked()? && registered()?.is_some_and(|v| v.eq_ignore_ascii_case(&command_for(&exe))))
}

pub fn enable() -> Result<String, String> {
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
