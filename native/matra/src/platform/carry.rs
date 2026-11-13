//! Physical desktop pointer/capture boundary. Every native call executes on Tauri's UI thread.
//! Windows capture belongs to the window-creating thread. X11 capture is GDK's own seat grab: GTK
//! selects XInput2 events, so a button press activates an XI2 implicit grab for GTK's client, and the
//! X server answers any grab of a different type (a core XGrabPointer, even from GTK's own
//! connection) with AlreadyGrabbed. Sampling borrows GDK's Xlib connection (Tauri's Linux
//! display_handle() opens a new, never-closed connection per call). Wayland never fabricates input.
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
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{cell::RefCell, ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void}};
    use tauri::Manager;
    const NOT_X11: &str = "Perimeter carry requires an X11 desktop. Wayland does not expose global pointer capture or absolute window placement; choose an X11 login session.";
    /// One window on GTK's own X11 client. `xdisplay` is BORROWED from GDK, which owns and
    /// closes it; this module never opens or closes an X connection. Do not use Tauri's
    /// `display_handle()` here: on Linux (tao 0.35.3) every call runs XOpenDisplay(NULL) and
    /// never closes the result, so each pointer sample leaked a new X client and fd.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    struct Target { gdk: usize, xdisplay: usize, window: c_ulong }
    #[derive(Clone)]
    struct Capture { target: Target, label: String }
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
        fn XDefaultRootWindow(display: *mut c_void) -> c_ulong;
        fn XQueryPointer(display: *mut c_void, window: c_ulong, root: *mut c_ulong, child: *mut c_ulong,
            root_x: *mut c_int, root_y: *mut c_int, win_x: *mut c_int, win_y: *mut c_int, mask: *mut c_uint) -> c_int;
        fn XQueryKeymap(display: *mut c_void, keys: *mut c_char) -> c_int;
        fn XKeysymToKeycode(display: *mut c_void, keysym: c_ulong) -> u8;
        fn XGetWindowAttributes(display: *mut c_void, window: c_ulong, attributes: *mut Attributes) -> c_int;
    }
    // GTK 3 / GDK 3 are already linked by Tauri's WebKitGTK runtime; these are their C entry points.
    #[link(name = "gdk-3")]
    extern "C" {
        fn gdk_display_get_default() -> *mut c_void;
        fn gdk_x11_display_get_type() -> usize;
        fn gdk_x11_display_get_xdisplay(display: *mut c_void) -> *mut c_void;
        fn gdk_x11_window_lookup_for_display(display: *mut c_void, window: c_ulong) -> *mut c_void;
        fn gdk_x11_display_error_trap_push(display: *mut c_void);
        fn gdk_x11_display_error_trap_pop(display: *mut c_void) -> c_int;
        // GDK 3.20+ capture (gdk-sys 0.18.2 signatures). On X11 with XI2 the seat grab is an
        // XIGrabDevice of the master pointer from GTK's client, which replaces its implicit grab.
        fn gdk_display_get_default_seat(display: *mut c_void) -> *mut c_void;
        fn gdk_seat_get_pointer(seat: *mut c_void) -> *mut c_void;
        fn gdk_seat_grab(seat: *mut c_void, window: *mut c_void, capabilities: c_uint, owner_events: c_int,
            cursor: *mut c_void, event: *const c_void, prepare: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void)>,
            prepare_data: *mut c_void) -> c_int;
        // Pointer-only release: gdk_seat_ungrab would also drop a keyboard grab this module never took.
        fn gdk_device_ungrab(device: *mut c_void, time: u32);
        fn gdk_display_device_is_grabbed(display: *mut c_void, device: *mut c_void) -> c_int;
        fn gdk_device_grab_info_libgtk_only(display: *mut c_void, device: *mut c_void,
            grab_window: *mut *mut c_void, owner_events: *mut c_int) -> c_int;
        fn gdk_window_is_visible(window: *mut c_void) -> c_int;
    }
    const GDK_SEAT_CAPABILITY_POINTER: c_uint = 1;
    #[link(name = "gobject-2.0")]
    extern "C" { fn g_type_check_instance_is_a(instance: *mut c_void, type_: usize) -> c_int; }

    /// GDK treats an untrapped X error on its connection as fatal to the process. Every
    /// request this module makes on the borrowed connection runs inside a trap; the pop
    /// synchronises and returns the first X error code (0 = none).
    fn trapped<T>(gdk: usize, request: impl FnOnce() -> T) -> (T, c_int) {
        unsafe { gdk_x11_display_error_trap_push(gdk as *mut c_void) };
        let value = request();
        (value, unsafe { gdk_x11_display_error_trap_pop(gdk as *mut c_void) })
    }
    /// Runs on the UI thread only (GDK is single-threaded).
    fn target(app: &AppHandle, label: &str) -> Result<Target, String> {
        let gdk = unsafe { gdk_display_get_default() };
        if gdk.is_null() { return Err("GTK has no open desktop display.".into()); }
        // Never reinterpret a Wayland (or other backend) GdkDisplay as an Xlib Display.
        if unsafe { g_type_check_instance_is_a(gdk, gdk_x11_display_get_type()) } == 0 { return Err(NOT_X11.into()); }
        let window = app.get_webview_window(label).ok_or("The carry window no longer exists.")?;
        // tao resolves this through gdk_x11_window_get_xid on the realized GdkWindow; it opens no connection.
        let handle = window.window_handle().map_err(|_| "Desktop window handle is unavailable.")?;
        let RawWindowHandle::Xlib(handle) = handle.as_raw() else { return Err(NOT_X11.into()) };
        // The XID must name a window GDK itself manages on this display (identity, not just a number).
        if handle.window == 0 || unsafe { gdk_x11_window_lookup_for_display(gdk, handle.window) }.is_null() {
            return Err("The carry window is not a GTK window on the desktop's X11 display.".into());
        }
        let xdisplay = unsafe { gdk_x11_display_get_xdisplay(gdk) };
        if xdisplay.is_null() { return Err("GTK's X11 display connection is unavailable.".into()); }
        let target = Target { gdk: gdk as usize, xdisplay: xdisplay as usize, window: handle.window };
        // Diagnostic, bounded: logged once, and again only if the connection identity ever changes.
        static LOGGED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        if LOGGED.swap(target.xdisplay, std::sync::atomic::Ordering::Relaxed) != target.xdisplay {
            crate::applog(&format!("carry: X11 pointer uses GTK's display connection {:#x} (borrowed; never opened or closed here)", target.xdisplay));
        }
        Ok(target)
    }
    /// GDK's live window for the target XID, looked up afresh (null once GDK destroyed it), so no
    /// GdkWindow pointer outlives a call. UI thread only.
    fn gdk_window(target: Target) -> *mut c_void {
        unsafe { gdk_x11_window_lookup_for_display(target.gdk as *mut c_void, target.window) }
    }
    fn pointer_device(gdk: usize) -> *mut c_void {
        let seat = unsafe { gdk_display_get_default_seat(gdk as *mut c_void) };
        if seat.is_null() { std::ptr::null_mut() } else { unsafe { gdk_seat_get_pointer(seat) } }
    }
    /// GDK's steady-state grab for the master pointer is this module's explicit grab on `window`:
    /// not an implicit button grab, and not a later in-process grab (menu, DnD) on another window.
    fn owns_grab(gdk: usize, pointer: *mut c_void, window: *mut c_void) -> bool {
        if pointer.is_null() || window.is_null() { return false; }
        let explicit = unsafe { gdk_display_device_is_grabbed(gdk as *mut c_void, pointer) } != 0;
        let mut grab_window: *mut c_void = std::ptr::null_mut();
        let known = unsafe { gdk_device_grab_info_libgtk_only(gdk as *mut c_void, pointer, &mut grab_window, std::ptr::null_mut()) } != 0;
        super::x11::grab_is_ours(explicit, known.then_some(grab_window as usize), window as usize)
    }
    pub fn begin(app: &AppHandle, label: &str) -> Result<(), String> {
        end()?;
        let target = target(app, label)?;
        let window = gdk_window(target);
        if window.is_null() { return Err("The carry window is not a GTK window on the desktop's X11 display.".into()); }
        // gdk_seat_grab reports a hidden window as a g_critical; X would answer GrabNotViewable.
        if unsafe { gdk_window_is_visible(window) } == 0 { return Err(super::x11::grab_failure(3, 0)); }
        let seat = unsafe { gdk_display_get_default_seat(target.gdk as *mut c_void) };
        let pointer = if seat.is_null() { std::ptr::null_mut() } else { unsafe { gdk_seat_get_pointer(seat) } };
        if pointer.is_null() { return Err("GTK reports no pointer device on this X11 seat.".into()); }
        // GTK's own grab on GTK's own client and grab type: it supersedes the XI2 implicit grab from
        // the grip press (GtkMenu and DnD take the same path during a press). Pointer only, no owner
        // events (events report to the carry window), no cursor change, async modes, current time.
        let (status, error) = trapped(target.gdk, || unsafe {
            gdk_seat_grab(seat, window, GDK_SEAT_CAPABILITY_POINTER, 0, std::ptr::null_mut(), std::ptr::null(), None, std::ptr::null_mut())
        });
        if error != 0 || status != 0 {
            // A refused grab took nothing, and GTK's implicit grab stays with GTK. Release only a
            // grab GDK recorded as this module's own (a success later reported with an X error).
            if status == 0 && owns_grab(target.gdk, pointer, window) {
                let _ = trapped(target.gdk, || unsafe { gdk_device_ungrab(pointer, 0) });
            }
            return Err(super::x11::grab_failure(status, error));
        }
        CAPTURE.with(|slot| *slot.borrow_mut() = Some(Capture { target, label: label.into() }));
        Ok(())
    }
    pub fn end() -> Result<(), String> {
        if let Some(capture) = CAPTURE.with(|slot| slot.borrow_mut().take()) {
            // A replaced default display means the connection the grab lived on is gone with it.
            if unsafe { gdk_display_get_default() } as usize == capture.target.gdk {
                // A destroyed window already ended its grab in both X and GDK. Never release a
                // grab this module does not own (an in-process menu or DnD that superseded it).
                let window = gdk_window(capture.target);
                let pointer = pointer_device(capture.target.gdk);
                if owns_grab(capture.target.gdk, pointer, window) {
                    // GDK flushes the XIUngrabDevice; any X error is trapped.
                    let _ = trapped(capture.target.gdk, || unsafe { gdk_device_ungrab(pointer, 0) });
                }
            }
        }
        Ok(())
    }
    pub fn sample(app: &AppHandle) -> Result<PointerSample, String> {
        let active = CAPTURE.with(|slot| slot.borrow().clone());
        let target = target(app, active.as_ref().map(|c| c.label.as_str()).unwrap_or("notch"))?;
        let display = target.xdisplay as *mut c_void;
        let (mut root, mut child, mut x, mut y, mut local_x, mut local_y, mut mask) = (0, 0, 0, 0, 0, 0, 0);
        let mut keys = [0 as c_char; 32];
        let mut attributes: Attributes = unsafe { std::mem::zeroed() };
        let ((described, pointer, keymap, escape), _error) = trapped(target.gdk, || unsafe {
            // An X11 active grab persists until explicitly released or its window becomes unviewable.
            // Another X client cannot steal it. Verify the same mapped window on each sample.
            let described = XGetWindowAttributes(display, target.window, &mut attributes) != 0;
            let screen_root = if described { attributes.root } else { XDefaultRootWindow(display) };
            let pointer = XQueryPointer(display, screen_root, &mut root, &mut child, &mut x, &mut y, &mut local_x, &mut local_y, &mut mask) != 0;
            let keymap = XQueryKeymap(display, keys.as_mut_ptr()) != 0;
            (described, pointer, keymap, XKeysymToKeycode(display, 0xff1b) as usize)
        });
        // A BadWindow from the attribute query (window destroyed) is expected and means "not viewable".
        if !pointer { return Err("X11 cannot sample this desktop's pointer screen. Carry was cancelled.".into()); }
        if !keymap { return Err("X11 keyboard state is unavailable.".into()); }
        let viewable = described && attributes.map_state == 2;
        // X ends the grab when its window becomes unviewable; GDK tracks that and any in-process
        // grab that supersedes this one. Only meaningful while a capture is held.
        let owned = active.is_some() && owns_grab(target.gdk, pointer_device(target.gdk), gdk_window(target));
        Ok(PointerSample { x: x as f64, y: y as f64, left_down: mask & (1 << 8) != 0,
            escape_down: super::x11::key_down(&keys.map(|k| k as u8), escape),
            capture_lost: super::x11::capture_lost(active.map(|c| c.target), target, viewable, owned) })
    }
}

/// Platform-independent X11 decisions, kept separate so host tests cover them.
#[cfg(any(test, target_os = "linux"))]
mod x11 {
    /// A grab belongs to one window on one client connection. A different window, a different
    /// connection, an unviewable window, or GDK no longer recording it as ours means it is gone.
    pub fn capture_lost<T: PartialEq>(active: Option<T>, current: T, viewable: bool, owned: bool) -> bool {
        active.is_some_and(|held| held != current || !viewable || !owned)
    }
    /// GDK's last pointer grab is ours only when it is explicit (not an implicit button grab)
    /// and its window is the carry window.
    pub fn grab_is_ours<W: PartialEq>(explicit: bool, grab_window: Option<W>, ours: W) -> bool {
        explicit && grab_window.is_some_and(|window| window == ours)
    }
    /// XQueryKeymap bit vector; keycode 0 means the keysym has no key.
    pub fn key_down(keys: &[u8; 32], keycode: usize) -> bool {
        keycode != 0 && keycode < 256 && keys[keycode / 8] & (1 << (keycode % 8)) != 0
    }
    pub fn grab_failure(status: i32, error: i32) -> String {
        if error != 0 { return format!("X11 rejected pointer capture (X error {error}); the carry window may have closed."); }
        let reason = match status {
            1 => "AlreadyGrabbed: another client or an incompatible grab owns the pointer",
            2 => "GrabInvalidTime",
            3 => "GrabNotViewable: the carry window is not mapped",
            4 => "GrabFrozen: another grab froze the pointer",
            5 => "GrabFailed: GTK could not grab the pointer",
            _ => "unknown status",
        };
        format!("X11 refused pointer capture (status {status}, {reason}).")
    }
}

#[cfg(test)]
mod tests {
    use super::x11::*;
    #[derive(Clone, Copy, PartialEq, Debug)]
    struct Id { display: usize, window: u64 }
    const HELD: Id = Id { display: 0x1000, window: 42 };
    #[test]
    fn same_connection_and_window_keeps_capture() {
        assert!(!capture_lost(Some(HELD), HELD, true, true));
        // A stable borrowed connection is what makes this hold across samples; a fresh
        // connection per sample (the old display_handle path) reported loss on every sample.
        assert!(capture_lost(Some(HELD), Id { display: 0x2000, ..HELD }, true, true));
    }
    #[test]
    fn other_window_or_unviewable_loses_capture() {
        assert!(capture_lost(Some(HELD), Id { window: 43, ..HELD }, true, true));
        assert!(capture_lost(Some(HELD), HELD, false, true));
    }
    #[test]
    fn grab_no_longer_owned_loses_capture() {
        assert!(capture_lost(Some(HELD), HELD, true, false));
    }
    #[test]
    fn idle_sampling_never_reports_loss() {
        assert!(!capture_lost(None, HELD, false, false));
        assert!(!capture_lost(None, HELD, true, false));
        assert!(!capture_lost(None, HELD, true, true));
    }
    #[test]
    fn only_an_explicit_grab_on_the_carry_window_is_ours() {
        assert!(grab_is_ours(true, Some(7usize), 7));
        // GTK's implicit button grab, even on the carry window, is not this module's capture.
        assert!(!grab_is_ours(false, Some(7usize), 7));
        // A later in-process grab (menu, DnD) on another window superseded ours.
        assert!(!grab_is_ours(true, Some(8usize), 7));
        assert!(!grab_is_ours(true, None, 7usize));
    }
    #[test]
    fn keymap_bits() {
        let mut keys = [0u8; 32];
        keys[9 / 8] |= 1 << (9 % 8); // Escape is keycode 9 on evdev layouts
        assert!(key_down(&keys, 9));
        assert!(!key_down(&keys, 10));
        assert!(!key_down(&keys, 0));
        assert!(!key_down(&keys, 256));
        keys[0] = 0xff;
        assert!(!key_down(&keys, 0), "keycode 0 means no key even if bit 0 is set");
    }
    #[test]
    fn grab_failures_name_the_cause() {
        assert!(grab_failure(1, 0).contains("AlreadyGrabbed"));
        assert!(grab_failure(3, 0).contains("GrabNotViewable"));
        assert!(grab_failure(5, 0).contains("GrabFailed"));
        assert!(grab_failure(0, 3).contains("X error 3"));
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod native {
    use super::*;
    pub fn begin(_: &AppHandle, _: &str) -> Result<(), String> { Err("The Mac product uses the native Swift carry implementation.".into()) }
    pub fn end() -> Result<(), String> { Ok(()) }
    pub fn sample(_: &AppHandle) -> Result<PointerSample, String> { Err("The Mac product uses the native Swift pointer adapter.".into()) }
}
