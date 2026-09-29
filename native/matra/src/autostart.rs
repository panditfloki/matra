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
        if status.0 == 2 {
            return Ok(None);
        }
        status.ok().map_err(|e| e.to_string())?;
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
    Ok(read(APPROVED_KEY, RRF_RT_REG_BINARY)?.is_some_and(|b| matches!(b.first(), Some(3 | 7))))
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

pub fn is_enabled() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    matches!(blocked(), Ok(false))
        && registered()
            .ok()
            .flatten()
            .is_some_and(|v| v.eq_ignore_ascii_case(&command_for(&exe)))
}

pub fn enable() -> Result<String, String> {
    if blocked()? {
        return Err("Windows disabled startup. Enable Matra in Windows Settings > Apps > Startup, then try again.".into());
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(existing) = registered()? {
        if !crate::hooks_install::owned_executable(&existing, &exe) {
            return Err(
                "Another installation owns the Matra startup entry; it was left untouched.".into(),
            );
        }
    }
    write_command(&command_for(&exe))?;
    if !is_enabled() {
        return Err("Startup registration could not be verified.".into());
    }
    Ok("Start at login enabled for this installed Matra.".into())
}

/// Retarget only an existing owned entry; never override the Windows disabled state.
pub fn migrate() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if registered()?.is_some_and(|v| crate::hooks_install::owned_executable(&v, &exe)) {
        write_command(&command_for(&exe))?;
    }
    Ok(())
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
}
