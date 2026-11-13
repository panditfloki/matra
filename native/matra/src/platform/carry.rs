//! Physical desktop pointer/capture boundary. Every native call executes on Tauri's UI thread.
//! Windows capture belongs to the window-creating thread. X11 uses GTK's display connection
//! (a second X client cannot replace GTK's implicit button grab). Wayland never fabricates input.
use tauri::AppHandle;

#[derive(Clone, Copy, Debug)]
pub struct PointerSample {
    pub x: f64,
    pub y: f64,
    pub left_down: bool,
    pub escape_down: bool,
    pub capture_lost: bool,
}

fn on_ui<T: Send + 'static>(app: &AppHandle, f: impl FnOnce(AppHandle) -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let handle = app.clone();
    app.run_on_main_thread(move || { let _ = send.send(f(handle)); })
        .map_err(|_| "The desktop UI thread is unavailable.".to_string())?;
    receive.recv_timeout(std::time::Duration::from_secs(2))
        .map_err(|_| "The desktop UI thread did not respond to the pointer request.".to_string())?
}

pub fn begin(app: &AppHandle, window_label: &str) -> Result<(), String> {
    let label = window_label.to_owned();
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pending = cancelled.clone();
    let result = on_ui(app, move |handle| {
        use std::sync::atomic::Ordering::SeqCst;
        if pending.load(SeqCst) { return Err("Pointer capture request expired.".into()); }
        let result = native::begin(&handle, &label);
        if pending.load(SeqCst) { let _ = native::end(); return Err("Pointer capture request expired.".into()); }
        result
    });
    if result.is_err() {
        cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
        // A timeout can race with a completed native grab. Queue unconditional cleanup after it.
        let _ = app.run_on_main_thread(|| { let _ = native::end(); });
    }
    result
}

pub fn end(app: &AppHandle) -> Result<(), String> { on_ui(app, |_| native::end()) }
pub fn sample(app: &AppHandle) -> Result<PointerSample, String> { on_ui(app, |handle| native::sample(&handle)) }

#[cfg(windows)]
mod native {
    use super::*;
    use std::cell::Cell;
    use tauri::Manager;
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetCapture, ReleaseCapture, SetCapture, VK_ESCAPE, VK_LBUTTON};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    thread_local! { static CAPTURE: Cell<isize> = const { Cell::new(0) }; }
    pub fn begin(app: &AppHandle, label: &str) -> Result<(), String> {
        end()?;
        let window = app.get_webview_window(label).ok_or("The carry window no longer exists.")?;
        let hwnd = HWND(window.hwnd().map_err(|_| "Cannot obtain the carry window handle.")?.0);
        unsafe {
            SetCapture(hwnd);
            if GetCapture() != hwnd { return Err("Windows refused pointer capture. Keep the mouse button pressed and retry.".into()); }
        }
        CAPTURE.with(|slot| slot.set(hwnd.0 as isize));
        Ok(())
    }
    pub fn end() -> Result<(), String> {
        let previous = CAPTURE.with(|slot| slot.replace(0));
        if previous != 0 && unsafe { GetCapture().0 as isize } == previous {
            unsafe { ReleaseCapture() }.map_err(|_| "Windows could not release pointer capture.".to_string())?;
        }
        Ok(())
    }
    pub fn sample(_app: &AppHandle) -> Result<PointerSample, String> {
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point) }.map_err(|_| "Windows desktop pointer is unavailable.".to_string())?;
        let expected = CAPTURE.with(Cell::get);
        Ok(PointerSample { x: point.x as f64, y: point.y as f64,
            left_down: unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0,
            escape_down: unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) } < 0,
            capture_lost: expected != 0 && unsafe { GetCapture().0 as isize } != expected })
    }
}

#[cfg(target_os = "linux")]
mod native {
    use super::*;
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle};
    use std::{cell::RefCell, ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void}};
    use tauri::Manager;
    #[derive(Clone)]
    struct Capture { display: usize, window: c_ulong, root: c_ulong, label: String }
    thread_local! { static CAPTURE: RefCell<Option<Capture>> = const { RefCell::new(None) }; }
    #[repr(C)]
    struct Attributes {
        x: c_int, y: c_int, width: c_int, height: c_int, border_width: c_int, depth: c_int,
        visual: *mut c_void, root: c_ulong, class: c_int, bit_gravity: c_int, win_gravity: c_int,
        backing_store: c_int, backing_planes: c_ulong, backing_pixel: c_ulong,
        save_under: c_int, colormap: c_ulong, map_installed: c_int, map_state: c_int,
        all_event_masks: c_long, your_event_mask: c_long, do_not_propagate_mask: c_long,
        override_redirect: c_int, screen: *mut c_void,
    }
    #[link(name = "X11")]
    extern "C" {
        fn XRootWindow(display: *mut c_void, screen: c_int) -> c_ulong;
        fn XGrabPointer(display: *mut c_void, window: c_ulong, owner_events: c_int, event_mask: c_uint,
            pointer_mode: c_int, keyboard_mode: c_int, confine_to: c_ulong, cursor: c_ulong, time: c_ulong) -> c_int;
        fn XUngrabPointer(display: *mut c_void, time: c_ulong) -> c_int;
        fn XFlush(display: *mut c_void) -> c_int;
        fn XQueryPointer(display: *mut c_void, window: c_ulong, root: *mut c_ulong, child: *mut c_ulong,
            root_x: *mut c_int, root_y: *mut c_int, win_x: *mut c_int, win_y: *mut c_int, mask: *mut c_uint) -> c_int;
        fn XQueryKeymap(display: *mut c_void, keys: *mut c_char) -> c_int;
        fn XKeysymToKeycode(display: *mut c_void, keysym: c_ulong) -> u8;
        fn XGetWindowAttributes(display: *mut c_void, window: c_ulong, attributes: *mut Attributes) -> c_int;
    }
    fn handles(app: &AppHandle, label: &str) -> Result<Capture, String> {
        let window = app.get_webview_window(label).ok_or("The carry window no longer exists.")?;
        let display = window.display_handle().map_err(|_| "Desktop display handle is unavailable.")?;
        let handle = window.window_handle().map_err(|_| "Desktop window handle is unavailable.")?;
        match (display.as_raw(), handle.as_raw()) {
            (RawDisplayHandle::Xlib(display), RawWindowHandle::Xlib(window)) => {
                let pointer = display.display.ok_or("X11 display connection is unavailable.")?.as_ptr();
                Ok(Capture { display: pointer as usize, window: window.window, root: unsafe { XRootWindow(pointer, display.screen) }, label: label.into() })
            }
            _ => Err("Perimeter carry requires an X11 desktop. Wayland does not expose global pointer capture or absolute window placement; choose an X11 login session.".into()),
        }
    }
    pub fn begin(app: &AppHandle, label: &str) -> Result<(), String> {
        end()?;
        let capture = handles(app, label)?;
        let display = capture.display as *mut c_void;
        // Async modes keep the desktop responsive. No confinement and no keyboard grab.
        let code = unsafe { XGrabPointer(display, capture.window, 0, (1 << 2) | (1 << 3) | (1 << 6), 1, 1, 0, 0, 0) };
        if code != 0 { return Err(format!("X11 refused pointer capture (status {code}); another desktop operation may own it.")); }
        unsafe { XFlush(display); }
        CAPTURE.with(|slot| *slot.borrow_mut() = Some(capture));
        Ok(())
    }
    pub fn end() -> Result<(), String> {
        if let Some(capture) = CAPTURE.with(|slot| slot.borrow_mut().take()) {
            unsafe { XUngrabPointer(capture.display as *mut c_void, 0); XFlush(capture.display as *mut c_void); }
        }
        Ok(())
    }
    pub fn sample(app: &AppHandle) -> Result<PointerSample, String> {
        let active = CAPTURE.with(|slot| slot.borrow().clone());
        let handle = handles(app, active.as_ref().map(|c| c.label.as_str()).unwrap_or("notch"))?;
        let display = handle.display as *mut c_void;
        let (mut root, mut child, mut x, mut y, mut local_x, mut local_y, mut mask) = (0, 0, 0, 0, 0, 0, 0);
        if unsafe { XQueryPointer(display, handle.root, &mut root, &mut child, &mut x, &mut y, &mut local_x, &mut local_y, &mut mask) } == 0 {
            return Err("X11 cannot sample this desktop's pointer screen. Carry was cancelled.".into());
        }
        let mut keys = [0 as c_char; 32];
        if unsafe { XQueryKeymap(display, keys.as_mut_ptr()) } == 0 { return Err("X11 keyboard state is unavailable.".into()); }
        let escape = unsafe { XKeysymToKeycode(display, 0xff1b) } as usize;
        let mut attributes: Attributes = unsafe { std::mem::zeroed() };
        // An X11 active grab persists until explicitly released or its window becomes unviewable.
        // Another X client cannot steal it. Verify the same mapped window on each sample.
        let visible = unsafe { XGetWindowAttributes(display, handle.window, &mut attributes) } != 0 && attributes.map_state == 2;
        let capture_lost = active.is_some_and(|c| c.window != handle.window || c.display != handle.display || !visible);
        Ok(PointerSample { x: x as f64, y: y as f64, left_down: mask & (1 << 8) != 0,
            escape_down: escape != 0 && (keys[escape / 8] as u8 & (1 << (escape % 8))) != 0, capture_lost })
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod native {
    use super::*;
    pub fn begin(_: &AppHandle, _: &str) -> Result<(), String> { Err("The Mac product uses the native Swift carry implementation.".into()) }
    pub fn end() -> Result<(), String> { Ok(()) }
    pub fn sample(_: &AppHandle) -> Result<PointerSample, String> { Err("The Mac product uses the native Swift pointer adapter.".into()) }
}
