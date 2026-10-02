//! Keep the Windows notch above application windows without activating it.
//!
//! Tao caches its always-on-top flag: setting true again does nothing when the
//! cache already says true. Read the HWND instead so native demotion is repaired.
//! Another topmost application can also be raised above an otherwise topmost
//! notch. Repair that overlap, while leaving popup menus above their owner.

#[cfg(windows)]
mod native {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
    use windows::Win32::UI::WindowsAndMessaging::*;

    fn rect(window: HWND) -> Option<RECT> {
        let mut value = RECT::default();
        unsafe { GetWindowRect(window, &mut value).ok()? };
        Some(value)
    }

    fn overlaps(a: &RECT, b: &RECT) -> bool {
        a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
    }

    #[derive(Default)]
    struct Stacking {
        covered: bool,
        popup: bool,
    }

    unsafe fn stacking(window: HWND) -> Stacking {
        let mut state = Stacking::default();
        let Some(bounds) = rect(window) else {
            return state;
        };
        let mut previous = GetWindow(window, GW_HWNDPREV).unwrap_or_default();
        // HWNDs can disappear or be reordered while we inspect them. Bound the
        // traversal so this optional repair can never stall the UI thread.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..256 {
            if previous.is_invalid() || !seen.insert(previous.0 as usize) {
                break;
            }
            if GetWindowLongW(previous, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 == 0 {
                break;
            }
            // A menu must remain above the notch it belongs to. System popup
            // menus also need their normal stacking while the user chooses.
            let mut class = [0u16; 32];
            let length = GetClassNameW(previous, &mut class) as usize;
            let class = String::from_utf16_lossy(&class[..length]);
            let menu = class == "#32768";
            let taskbar = matches!(class.as_str(), "Shell_TrayWnd" | "Shell_SecondaryTrayWnd");
            let owned = GetWindow(previous, GW_OWNER).unwrap_or_default() == window;
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(
                previous,
                DWMWA_CLOAKED,
                (&mut cloaked as *mut u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
            if !taskbar
                && cloaked == 0
                && IsWindowVisible(previous).as_bool()
                && !IsIconic(previous).as_bool()
                && rect(previous).is_some_and(|other| overlaps(&bounds, &other))
            {
                if owned || menu {
                    state.popup = true;
                } else {
                    state.covered = true;
                }
            }
            previous = GetWindow(previous, GW_HWNDPREV).unwrap_or_default();
        }
        state
    }

    /// Returns true only when native stacking needed and received a repair.
    /// Must run on the window's owning thread; geometry, visibility and focus
    /// are deliberately excluded from the SetWindowPos operation.
    pub(super) fn ensure(window: HWND, force: bool) -> windows::core::Result<bool> {
        unsafe {
            if !IsWindowVisible(window).as_bool() || IsIconic(window).as_bool() {
                return Ok(false);
            }
            let mut gui = GUITHREADINFO {
                cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                ..Default::default()
            };
            if GetGUIThreadInfo(0, &mut gui).is_ok() && (gui.flags & GUI_INMENUMODE).0 != 0 {
                return Ok(false);
            }
            let state = stacking(window);
            if state.popup {
                return Ok(false);
            }
            let topmost = GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0 != 0;
            if !force && topmost && !state.covered {
                return Ok(false);
            }
            SetWindowPos(
                window,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
            )?;
            Ok(true)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use windows::core::w;

        struct TestWindow(HWND);
        impl TestWindow {
            unsafe fn new(topmost: bool) -> Self {
                let mut style = WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
                if topmost {
                    style |= WS_EX_TOPMOST;
                }
                Self(
                    CreateWindowExW(
                        style,
                        w!("STATIC"),
                        w!("Matra stacking fixture"),
                        WS_POPUP,
                        -30000,
                        -30000,
                        100,
                        100,
                        None,
                        None,
                        None,
                        None,
                    )
                    .unwrap(),
                )
            }
        }
        impl Drop for TestWindow {
            fn drop(&mut self) {
                unsafe {
                    let _ = DestroyWindow(self.0);
                }
            }
        }

        #[test]
        fn restores_native_demotion_and_overlap_without_focus_or_geometry_changes() {
            unsafe {
                let notch = TestWindow::new(true);
                let cover = TestWindow::new(true);
                let foreground = GetForegroundWindow();
                let _ = ShowWindow(notch.0, SW_SHOWNOACTIVATE);
                let before = rect(notch.0).unwrap();
                SetWindowPos(
                    notch.0,
                    HWND_NOTOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
                .unwrap();
                assert_eq!(
                    GetWindowLongW(notch.0, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0,
                    0
                );
                assert!(ensure(notch.0, false).unwrap());
                assert_ne!(
                    GetWindowLongW(notch.0, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0,
                    0
                );
                assert!(
                    !ensure(notch.0, false).unwrap(),
                    "healthy notch should not be raised repeatedly"
                );

                let _ = ShowWindow(cover.0, SW_SHOWNOACTIVATE);
                SetWindowPos(
                    cover.0,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
                .unwrap();
                assert!(
                    stacking(notch.0).covered,
                    "topmost fixture must cover the notch in native Z order"
                );
                assert!(ensure(notch.0, false).unwrap());
                assert!(!stacking(notch.0).covered);
                assert_eq!(
                    GetForegroundWindow(),
                    foreground,
                    "repair must not steal keyboard focus"
                );
                let after = rect(notch.0).unwrap();
                assert_eq!(
                    (before.left, before.top, before.right, before.bottom),
                    (after.left, after.top, after.right, after.bottom)
                );
                assert_ne!(
                    GetWindowLongW(notch.0, GWL_EXSTYLE) as u32 & WS_EX_NOACTIVATE.0,
                    0
                );

                // An owned popup must remain in front even if a second app is
                // also covering the notch. Forced show repairs defer too.
                let popup = TestWindow::new(true);
                SetWindowLongPtrW(popup.0, GWLP_HWNDPARENT, notch.0 .0 as isize);
                let _ = ShowWindow(popup.0, SW_SHOWNOACTIVATE);
                SetWindowPos(
                    cover.0,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
                .unwrap();
                assert!(stacking(notch.0).popup);
                assert!(stacking(notch.0).covered);
                assert!(
                    !ensure(notch.0, true).unwrap(),
                    "repair must defer while an owned popup is open"
                );
                let _ = ShowWindow(popup.0, SW_HIDE);
                assert!(ensure(notch.0, false).unwrap());

                let _ = ShowWindow(notch.0, SW_HIDE);
                SetWindowPos(
                    notch.0,
                    HWND_NOTOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
                .unwrap();
                assert!(
                    !ensure(notch.0, true).unwrap(),
                    "hidden notch must stay hidden"
                );
                assert!(!IsWindowVisible(notch.0).as_bool());
                let _ = ShowWindow(notch.0, SW_SHOWNOACTIVATE);
                assert!(
                    ensure(notch.0, true).unwrap(),
                    "showing the notch must restore topmost again"
                );
                assert_ne!(
                    GetWindowLongW(notch.0, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0,
                    0
                );
            }
        }
    }
}

pub fn restore(app: &tauri::AppHandle) {
    #[cfg(windows)]
    {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || check(&handle, true));
    }
    #[cfg(not(windows))]
    let _ = app;
}

#[cfg(windows)]
fn check(app: &tauri::AppHandle, force: bool) {
    use tauri::Manager;
    let Some(window) = app.get_webview_window("notch") else {
        return;
    };
    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd = windows::Win32::Foundation::HWND(hwnd.0);
    if let Err(error) = native::ensure(hwnd, force) {
        crate::applog(&format!("notch topmost repair failed: {error}"));
    }
}

pub fn start(app: tauri::AppHandle) {
    #[cfg(windows)]
    std::thread::spawn(move || {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        };
        let pending = Arc::new(AtomicBool::new(false));
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if pending.swap(true, Ordering::Relaxed) {
                continue;
            }
            let handle = app.clone();
            let done = pending.clone();
            if app
                .run_on_main_thread(move || {
                    check(&handle, false);
                    done.store(false, Ordering::Relaxed);
                })
                .is_err()
            {
                break;
            }
        }
    });
    #[cfg(not(windows))]
    let _ = app;
}
