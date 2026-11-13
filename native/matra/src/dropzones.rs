//! Actual notch body and cells in the hand, on a transparent, nonactivating overlay.
use crate::carry_session::{Frame, Presentation};
use std::sync::{
    atomic::{AtomicU64, Ordering::SeqCst},
    Mutex,
};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
const LABEL: &str = "dropzones";
static CURRENT: Mutex<Option<Scene>> = Mutex::new(None);
static READY: AtomicU64 = AtomicU64::new(0);
static REVEALED: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, serde::Serialize)]
pub struct Scene {
    pub presentation: Presentation,
    pub frame: Frame,
}

pub fn show(
    app: &AppHandle,
    screen: &crate::Screen,
    presentation: &Presentation,
    frame: &Frame,
) -> Result<(), String> {
    READY.store(0, SeqCst);
    REVEALED.store(0, SeqCst);
    *CURRENT.lock().unwrap() = Some(Scene {
        presentation: presentation.clone(),
        frame: frame.clone(),
    });
    if let Some(w) = app.get_webview_window(LABEL) {
        pin(&w, screen)?;
        w.show().map_err(|e| e.to_string())?;
        return w
            .emit("carry_scene", CURRENT.lock().unwrap().clone())
            .map_err(|e| e.to_string());
    }
    let (ax, ay, aw, ah) = screen.area();
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("dropzones.html".into()))
        .title("Matra carry")
        .position(ax as f64 / screen.scale, ay as f64 / screen.scale)
        .inner_size(aw as f64 / screen.scale, ah as f64 / screen.scale)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .focusable(false)
        .resizable(false)
        .build()
        .map_err(|e| e.to_string())?;
    pin(&w, screen)?;
    w.set_ignore_cursor_events(true)
        .map_err(|e| e.to_string())?;
    Ok(())
}
fn pin(w: &tauri::WebviewWindow, screen: &crate::Screen) -> Result<(), String> {
    let (ax, ay, aw, ah) = screen.area();
    let size = tauri::PhysicalSize::new(aw.max(1) as u32, ah.max(1) as u32);
    w.set_position(tauri::PhysicalPosition::new(ax, ay))
        .map_err(|e| e.to_string())?;
    w.set_size(size).map_err(|e| e.to_string())?;
    if w.outer_size().map(|s| s != size).unwrap_or(false) {
        w.set_size(size).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn relocate(app: &AppHandle, screen: &crate::Screen, frame: &Frame) -> Result<(), String> {
    let w = app
        .get_webview_window(LABEL)
        .ok_or("Carry overlay closed")?;
    pin(&w, screen)?;
    retarget(app, frame)
}
pub fn retarget(app: &AppHandle, frame: &Frame) -> Result<(), String> {
    if let Some(scene) = CURRENT.lock().unwrap().as_mut() {
        scene.frame = frame.clone();
    }
    app.emit_to(LABEL, "carry_frame", frame)
        .map_err(|e| e.to_string())
}
pub fn presentation(app: &AppHandle, p: &Presentation) -> Result<(), String> {
    if let Some(scene) = CURRENT.lock().unwrap().as_mut() {
        scene.presentation = p.clone();
    }
    app.emit_to(LABEL, "carry_presentation", p)
        .map_err(|e| e.to_string())
}
pub fn ready() -> u64 {
    READY.load(SeqCst)
}
pub fn revealed() -> u64 {
    REVEALED.load(SeqCst)
}
#[tauri::command]
pub fn carry_overlay_ready(id: u64) {
    if CURRENT
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|s| s.frame.id == id)
    {
        READY.store(id, SeqCst);
    }
}
#[tauri::command]
pub fn carry_revealed(id: u64) {
    REVEALED.store(id, SeqCst);
}
pub fn hide(app: &AppHandle) {
    *CURRENT.lock().unwrap() = None;
    READY.store(0, SeqCst);
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
}
#[tauri::command]
pub fn get_zones() -> Option<Scene> {
    CURRENT.lock().unwrap().clone()
}
