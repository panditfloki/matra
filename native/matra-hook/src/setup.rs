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
fn request_close(pid: u32) -> Result<(), String> {
    // Tauri's user windows use this class, including a hidden notch. Its Tao
    // event target and tray/IME windows must remain alive to deliver app.exit.
    request_close_with_class(pid, "Tauri Window")
}

#[cfg(windows)]
fn request_close_with_class(pid: u32, application_class: &str) -> Result<(), String> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
    };
    struct Request<'a> {
        pid: u32,
        application_class: &'a str,
        matched: bool,
        failed: bool,
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        let request = &mut *(data.0 as *mut Request<'_>);
        let mut owner = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut owner));
        // Never broadcast. OS UIPI remains in force; there is no message-filter bypass.
        if owner == request.pid {
            let mut class = [0u16; 256];
            let length = GetClassNameW(hwnd, &mut class);
            if length > 0
                && String::from_utf16_lossy(&class[..length as usize]) == request.application_class
            {
                request.matched = true;
                if PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0)).is_err() {
                    request.failed = true;
                }
            }
        }
        BOOL(1)
    }
    let mut request = Request {
        pid,
        application_class,
        matched: false,
        failed: false,
    };
    unsafe {
        EnumWindows(
            Some(visit),
            LPARAM(&mut request as *mut Request<'_> as isize),
        )
    }
    .map_err(|_| "Could not request a graceful Matra exit. Close Matra and retry.".to_string())?;
    if request.failed {
        return Err("Windows refused the close request. Close Matra yourself and retry.".into());
    }
    if !request.matched {
        return Err(
            "Could not locate Matra's application window. Close Matra yourself and retry.".into(),
        );
    }
    Ok(())
}

#[cfg(windows)]
fn wait_for_exit(
    process: windows::Win32::Foundation::HANDLE,
    milliseconds: u32,
) -> Result<(), String> {
    use windows::Win32::Foundation::WAIT_OBJECT_0;
    use windows::Win32::System::Threading::WaitForSingleObject;
    if unsafe { WaitForSingleObject(process, milliseconds) } == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err("Matra or an active hook did not exit. Close Matra, finish the active Claude operation, then retry. Nothing was force-closed.".into())
    }
}

#[cfg(windows)]
pub fn prepare(destination: &str) -> Result<(), String> {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, HANDLE, WAIT_OBJECT_0,
    };
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
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
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
                let opened = OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                    false,
                    entry.th32ProcessID,
                );
                if let Ok(raw) = opened {
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
                            if WaitForSingleObject(process.0, 0) == WAIT_OBJECT_0 {
                                next = Process32NextW(snapshot.0, &mut entry);
                                continue;
                            }
                            crate::setup_audit::record(
                                "prepare",
                                "owned-process-found",
                                Some(entry.th32ProcessID),
                            )?;
                            if name.eq_ignore_ascii_case("matra.exe") {
                                // WM_CLOSE is cooperative: older/unresponsive builds may decline.
                                // A short-lived hook has no GUI; let it finish rather than killing it.
                                let close = request_close(entry.th32ProcessID);
                                if close.is_err()
                                    && WaitForSingleObject(process.0, 0) != WAIT_OBJECT_0
                                {
                                    close?;
                                }
                                crate::setup_audit::record(
                                    "prepare",
                                    "close-requested",
                                    Some(entry.th32ProcessID),
                                )?;
                            }
                            let remaining = deadline
                                .saturating_duration_since(std::time::Instant::now())
                                .as_millis() as u32;
                            wait_for_exit(process.0, remaining)?;
                            crate::setup_audit::record(
                                "prepare",
                                "owned-process-exited",
                                Some(entry.th32ProcessID),
                            )?;
                        }
                    } else if WaitForSingleObject(process.0, 0) != WAIT_OBJECT_0 {
                        return Err(
                            "Cannot verify a running Matra process path. Close it and retry."
                                .into(),
                        );
                    }
                } else if let Err(error) = opened {
                    // A process can finish between snapshot and OpenProcess. All other
                    // failures are unknown ownership, not evidence that setup is ready.
                    if error.code() != windows::core::HRESULT::from_win32(ERROR_INVALID_PARAMETER.0)
                    {
                        return Err("Cannot inspect a running Matra process. Close it and retry; Windows permissions were not changed.".into());
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

    // Runs only when spawned by the parent test. No application, profile, hook
    // or startup registry is loaded. The hidden window always expires itself.
    #[cfg(windows)]
    #[test]
    fn close_window_fixture() {
        let Ok(mode) = std::env::var("MATRA_TEST_CLOSE_FIXTURE") else {
            return;
        };
        use std::io::Write;
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::*;
        unsafe {
            let window = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("Matra isolated close test"),
                WS_OVERLAPPED,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            // Another hidden top-level window in the same PID represents the
            // event-delivery infrastructure. It must not receive WM_CLOSE.
            let infrastructure = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("BUTTON"),
                w!("Isolated event target"),
                WS_OVERLAPPED,
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .unwrap();
            println!("MATRA_TEST_WINDOW_READY");
            std::io::stdout().flush().unwrap();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
            while IsWindow(window).as_bool() && std::time::Instant::now() < deadline {
                let mut message = MSG::default();
                while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    if message.message != WM_CLOSE || mode == "cooperative" {
                        DispatchMessageW(&message);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            if IsWindow(window).as_bool() {
                DestroyWindow(window).unwrap();
            }
            assert!(
                IsWindow(infrastructure).as_bool(),
                "setup closed a same-PID infrastructure window"
            );
            DestroyWindow(infrastructure).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn real_close_is_cooperative_and_timeout_does_not_kill() {
        use std::io::{BufRead, BufReader};
        use std::os::windows::{io::AsRawHandle, process::CommandExt};
        for mode in ["cooperative", "decline"] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "setup::tests::close_window_fixture",
                    "--nocapture",
                ])
                .env("MATRA_TEST_CLOSE_FIXTURE", mode)
                .creation_flags(0x08000000) // CREATE_NO_WINDOW; fixture GUI also remains hidden.
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let mut ready = false;
            let mut output = BufReader::new(child.stdout.take().unwrap());
            loop {
                let mut line = String::new();
                if output.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                if line.contains("MATRA_TEST_WINDOW_READY") {
                    ready = true;
                    break;
                }
            }
            assert!(ready, "isolated test window did not become ready");
            // Test the same class-scoped path using a built-in isolated class,
            // without loading the actual app or registering a runtime class.
            assert!(
                request_close(child.id()).is_err(),
                "fixture must not be mistaken for a Tauri app"
            );
            request_close_with_class(child.id(), "STATIC").unwrap();
            let handle = windows::Win32::Foundation::HANDLE(child.as_raw_handle());
            if mode == "cooperative" {
                wait_for_exit(handle, 2000).unwrap();
            } else {
                assert!(wait_for_exit(handle, 100).is_err());
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "declining process was killed"
                );
                // Let the fixture's own deadline close it, without any forced cleanup.
                wait_for_exit(handle, 5000).unwrap();
            }
            assert!(child.wait().unwrap().success());
        }
    }

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
