//! Read-only foreground fullscreen detection, scoped to the notch's monitor.
//! Wayland does not expose another client's active window state to this app.
use tauri::AppHandle;

pub fn available() -> bool {
    cfg!(windows) || (cfg!(target_os = "linux") && super::linux_x11_session())
}

fn covers(window: (i32, i32, i32, i32), screen: (i32, i32, i32, i32)) -> bool {
    let (x, y, w, h) = window;
    let (sx, sy, sw, sh) = screen;
    w > 0
        && h > 0
        && sw > 0
        && sh > 0
        && i64::from(x) <= i64::from(sx) + 2
        && i64::from(y) <= i64::from(sy) + 2
        && i64::from(x) + i64::from(w) >= i64::from(sx) + i64::from(sw) - 2
        && i64::from(y) + i64::from(h) >= i64::from(sy) + i64::from(sh) - 2
}

#[tauri::command]
pub async fn get_fullscreen_state(app: AppHandle) -> Option<bool> {
    if super::preview() || !available() {
        return None;
    }
    let screen = crate::target_screen(&app)?;
    tauri::async_runtime::spawn_blocking(move || native((screen.x, screen.y, screen.w, screen.h)))
        .await
        .ok()
        .flatten()
}

#[cfg(windows)]
fn native(screen: (i32, i32, i32, i32)) -> Option<bool> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetDesktopWindow, GetForegroundWindow, GetShellWindow, GetWindowLongW, GetWindowRect,
        GetWindowThreadProcessId, IsWindowVisible, GWL_STYLE, WS_CAPTION,
    };
    unsafe {
        let window = GetForegroundWindow();
        if window.0.is_null()
            || window == GetDesktopWindow()
            || window == GetShellWindow()
            || !IsWindowVisible(window).as_bool()
        {
            return Some(false);
        }
        // A maximized normal window can cover an auto-hidden taskbar. Its title frame still
        // distinguishes it from a borderless/fullscreen foreground window.
        if (GetWindowLongW(window, GWL_STYLE) as u32 & WS_CAPTION.0) != 0 {
            return Some(false);
        }
        let mut process = 0;
        GetWindowThreadProcessId(window, Some(&mut process));
        if process == std::process::id() {
            return Some(false);
        }
        let mut rect = RECT::default();
        GetWindowRect(window, &mut rect).ok()?;
        Some(covers(
            (
                rect.left,
                rect.top,
                rect.right.checked_sub(rect.left)?,
                rect.bottom.checked_sub(rect.top)?,
            ),
            screen,
        ))
    }
}

#[cfg(target_os = "linux")]
fn native(screen: (i32, i32, i32, i32)) -> Option<bool> {
    // A separate XCB client uses checked replies. A window disappearing mid-read is None,
    // never Xlib's process-wide fatal BadWindow handler. No GTK connection or event queue is touched.
    // ABI: https://xcb.freedesktop.org/manual/xproto_8h_source.html
    use std::ffi::{c_char, c_int, c_void};
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Cookie {
        sequence: u32,
    }
    #[repr(C)]
    struct Root {
        window: u32,
    }
    #[repr(C)]
    struct Roots {
        data: *mut Root,
        remaining: c_int,
        index: c_int,
    }
    #[repr(C)]
    struct Atom {
        response: u8,
        pad: u8,
        sequence: u16,
        length: u32,
        atom: u32,
    }
    #[repr(C)]
    struct Property {
        response: u8,
        format: u8,
        sequence: u16,
        length: u32,
        kind: u32,
        after: u32,
        count: u32,
        pad: [u8; 12],
    }
    #[repr(C)]
    struct Geometry {
        response: u8,
        depth: u8,
        sequence: u16,
        length: u32,
        root: u32,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
        border: u16,
        pad: [u8; 2],
    }
    #[repr(C)]
    struct Translation {
        response: u8,
        same_screen: u8,
        sequence: u16,
        length: u32,
        child: u32,
        x: i16,
        y: i16,
    }
    #[link(name = "xcb")]
    extern "C" {
        fn xcb_connect(display: *const c_char, screen: *mut c_int) -> *mut c_void;
        fn xcb_disconnect(c: *mut c_void);
        fn xcb_connection_has_error(c: *mut c_void) -> c_int;
        fn xcb_get_setup(c: *mut c_void) -> *const c_void;
        fn xcb_setup_roots_iterator(setup: *const c_void) -> Roots;
        fn xcb_screen_next(iterator: *mut Roots);
        fn xcb_intern_atom(c: *mut c_void, exists: u8, len: u16, name: *const c_char) -> Cookie;
        fn xcb_intern_atom_reply(
            c: *mut c_void,
            cookie: Cookie,
            error: *mut *mut c_void,
        ) -> *mut Atom;
        fn xcb_get_property(
            c: *mut c_void,
            delete: u8,
            window: u32,
            property: u32,
            kind: u32,
            offset: u32,
            length: u32,
        ) -> Cookie;
        fn xcb_get_property_reply(
            c: *mut c_void,
            cookie: Cookie,
            error: *mut *mut c_void,
        ) -> *mut Property;
        fn xcb_get_property_value(reply: *const Property) -> *mut c_void;
        fn xcb_get_geometry(c: *mut c_void, window: u32) -> Cookie;
        fn xcb_get_geometry_reply(
            c: *mut c_void,
            cookie: Cookie,
            error: *mut *mut c_void,
        ) -> *mut Geometry;
        fn xcb_translate_coordinates(c: *mut c_void, src: u32, dst: u32, x: i16, y: i16) -> Cookie;
        fn xcb_translate_coordinates_reply(
            c: *mut c_void,
            cookie: Cookie,
            error: *mut *mut c_void,
        ) -> *mut Translation;
        fn free(data: *mut c_void);
    }
    struct Connection(*mut c_void);
    impl Drop for Connection {
        fn drop(&mut self) {
            unsafe {
                if !self.0.is_null() {
                    xcb_disconnect(self.0);
                }
            }
        }
    }
    struct Reply<T>(*mut T);
    impl<T> Drop for Reply<T> {
        fn drop(&mut self) {
            unsafe {
                free(self.0.cast());
            }
        }
    }
    unsafe fn atom(c: *mut c_void, name: &[u8]) -> Option<u32> {
        let reply = Reply(xcb_intern_atom_reply(
            c,
            xcb_intern_atom(c, 1, name.len() as u16, name.as_ptr().cast()),
            std::ptr::null_mut(),
        ));
        reply.0.as_ref().map(|r| r.atom).filter(|v| *v != 0)
    }
    unsafe fn property(c: *mut c_void, window: u32, name: &[u8], kind: u32) -> Option<Vec<u32>> {
        let a = atom(c, name)?;
        let reply = Reply(xcb_get_property_reply(
            c,
            xcb_get_property(c, 0, window, a, kind, 0, 64),
            std::ptr::null_mut(),
        ));
        let info = reply.0.as_ref()?;
        if info.format != 32 || info.kind != kind || info.count > 64 || info.after != 0 {
            return None;
        }
        let ptr = xcb_get_property_value(reply.0).cast::<u32>();
        if info.count == 0 {
            return Some(Vec::new());
        }
        if ptr.is_null() {
            return None;
        }
        Some(std::slice::from_raw_parts(ptr, info.count as usize).to_vec())
    }
    unsafe {
        let mut number = 0;
        let connection = Connection(xcb_connect(std::ptr::null(), &mut number));
        let c = connection.0;
        if c.is_null() || xcb_connection_has_error(c) != 0 {
            return None;
        }
        let setup = xcb_get_setup(c);
        if setup.is_null() {
            return None;
        }
        let mut roots = xcb_setup_roots_iterator(setup);
        for _ in 0..number.max(0) {
            if roots.remaining <= 0 {
                return None;
            }
            xcb_screen_next(&mut roots);
        }
        if roots.remaining <= 0 || roots.data.is_null() {
            return None;
        }
        let root = (*roots.data).window;
        let active = *property(c, root, b"_NET_ACTIVE_WINDOW", 33)?.first()?;
        if active == 0 {
            return Some(false);
        }
        let states = property(c, active, b"_NET_WM_STATE", 4)?;
        let full = atom(c, b"_NET_WM_STATE_FULLSCREEN")?;
        if !states.contains(&full) {
            return Some(false);
        }
        let geometry = Reply(xcb_get_geometry_reply(
            c,
            xcb_get_geometry(c, active),
            std::ptr::null_mut(),
        ));
        let g = geometry.0.as_ref()?;
        let translation = Reply(xcb_translate_coordinates_reply(
            c,
            xcb_translate_coordinates(c, active, root, 0, 0),
            std::ptr::null_mut(),
        ));
        let t = translation.0.as_ref()?;
        if t.same_screen == 0 {
            return Some(false);
        }
        Some(covers(
            (
                i32::from(t.x),
                i32::from(t.y),
                i32::from(g.width),
                i32::from(g.height),
            ),
            screen,
        ))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
fn native(_: (i32, i32, i32, i32)) -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fullscreen_is_scoped_to_monitor_and_excludes_work_area_maximization() {
        let screen = (-1920, 0, 1920, 1080);
        assert!(covers((-1920, 0, 1920, 1080), screen));
        assert!(!covers((0, 0, 1920, 1080), screen));
        assert!(!covers((-1920, 0, 1920, 1040), screen));
        assert!(covers((-1921, -1, 1922, 1082), screen));
        assert!(!covers((0, 0, 0, 0), (0, 0, 0, 0)));
    }
}
