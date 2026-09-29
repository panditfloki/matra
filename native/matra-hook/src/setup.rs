//! Installer preparation without PowerShell or a change to OS security policy.
//! Only this installation and the product's legacy version folders are in scope.

fn normalized(path: &str) -> String {
    path.replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn owned_process(image: &str, destination: &str, legacy: &str) -> bool {
    let image = normalized(image);
    if image.split('\\').any(|part| matches!(part, "." | "..")) {
        return false;
    }
    let Some((parent, name)) = image.rsplit_once('\\') else {
        return false;
    };
    if !matches!(name, "matra.exe" | "matra-hook.exe") {
        return false;
    }
    if parent == normalized(destination) {
        return true;
    }
    let legacy = normalized(legacy);
    if legacy.is_empty() {
        return false;
    }
    parent == legacy
        || parent
            .strip_prefix(&format!("{legacy}\\"))
            .is_some_and(|version| !version.is_empty() && !version.contains('\\'))
}

#[cfg(not(windows))]
pub fn prepare(_: &str) -> Result<(), String> {
    Err("Installer preparation is only available on Windows.".into())
}

#[cfg(windows)]
pub fn prepare(destination: &str) -> Result<(), String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Diagnostics::ToolHelp::*;
    use windows::Win32::System::Threading::*;

    let path = std::path::Path::new(destination);
    if !path.is_absolute()
        || path.file_name().is_none()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err("Refusing an ambiguous installation directory.".into());
    }
    let legacy = std::env::var("LOCALAPPDATA")
        .map(|root| format!("{root}\\Programs\\MatraNotch"))
        .map_err(|_| "Cannot resolve the current user's installation directory.".to_string())?;
    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    unsafe {
        let snapshot =
            Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| e.to_string())?);
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut next = Process32FirstW(snapshot.0, &mut entry);
        while next.is_ok() {
            let name = String::from_utf16_lossy(
                &entry.szExeFile[..entry
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.szExeFile.len())],
            );
            if entry.th32ProcessID != std::process::id()
                && matches!(
                    name.to_ascii_lowercase().as_str(),
                    "matra.exe" | "matra-hook.exe"
                )
            {
                // Hold a handle through validation and exit, avoiding PID-reuse races.
                if let Ok(raw) = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
                    false,
                    entry.th32ProcessID,
                ) {
                    let process = Handle(raw);
                    let mut buffer = vec![0u16; 32768];
                    let mut length = buffer.len() as u32;
                    if QueryFullProcessImageNameW(
                        process.0,
                        PROCESS_NAME_WIN32,
                        PWSTR(buffer.as_mut_ptr()),
                        &mut length,
                    )
                    .is_ok()
                    {
                        let image = String::from_utf16_lossy(&buffer[..length as usize]);
                        if owned_process(&image, destination, &legacy) {
                            if TerminateProcess(process.0, 0).is_err()
                                && WaitForSingleObject(process.0, 0) != WAIT_OBJECT_0
                            {
                                return Err("Could not close this Matra installation. Close it and try again.".into());
                            }
                            if WaitForSingleObject(process.0, 30_000) != WAIT_OBJECT_0 {
                                return Err("This Matra installation did not exit in time.".into());
                            }
                        }
                    }
                }
            }
            next = Process32NextW(snapshot.0, &mut entry);
        }
        if let Err(error) = next {
            if error.code() != windows::core::HRESULT::from_win32(ERROR_NO_MORE_FILES.0) {
                return Err(format!("Could not enumerate running applications: {error}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_install_and_legacy_only() {
        let dest = r"C:\Users\पंडित\Apps\Mātrā";
        let legacy = r"C:\Users\पंडित\AppData\Local\Programs\MatraNotch";
        assert!(owned_process(&format!(r"{dest}\matra.exe"), dest, legacy));
        assert!(owned_process(
            &format!(r"{legacy}\1.8.4\matra.exe"),
            dest,
            legacy
        ));
        for foreign in [
            format!(r"{dest}-other\matra.exe"),
            format!(r"{dest}\uninstall.exe"),
            format!(r"{legacy}\1.8.4\nested\matra.exe"),
            format!(r"{legacy}\..\matra.exe"),
            r"D:\Other\matra.exe".into(),
        ] {
            assert!(!owned_process(&foreign, dest, legacy), "{foreign}");
        }
    }
}
