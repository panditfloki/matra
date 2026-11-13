//! Actual notch body and cells in the hand, on a transparent, nonactivating overlay.
use crate::carry_session::{Frame, Presentation};
use std::sync::{
    atomic::{AtomicU64, Ordering::SeqCst},
    Mutex,
};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
const LABEL: &str = "dropzones";
static CURRENT: Mutex<Option<Scene>> = Mutex::new(None);
static READY: AtomicU64 = AtomicU64::new(0);
static REVEALED: AtomicU64 = AtomicU64::new(0);
static PROGRESS: Mutex<Option<Progress>> = Mutex::new(None);

/// Stages the overlay page reports before its first-frame ACK (`carry_overlay_ready`).
const STAGES: [&str; 4] = ["script", "zones", "cells", "drawn"];
const MAX_TEXT: usize = 240;
const MAX_MARKS: usize = 8;

/// Bounded first-frame trace of one overlay scene. The ACK stays the only success signal; this
/// only lets a missing paint name where the page stopped, and a page failure end the carry now
/// with its own message instead of an anonymous deadline.
#[derive(Debug)]
struct Progress {
    id: u64,
    started: Instant,
    marks: Vec<(&'static str, u128)>,
    detail: Option<String>,
    error: Option<String>,
}
impl Progress {
    fn new(id: u64, started: Instant) -> Self {
        Progress {
            id,
            started,
            marks: Vec::new(),
            detail: None,
            error: None,
        }
    }
    /// `id == 0` is the page before it has received its scene (it cannot know the id yet).
    fn record(
        &mut self,
        id: u64,
        stage: &str,
        detail: Option<&str>,
        error: Option<&str>,
        at: Instant,
    ) -> bool {
        if id != 0 && id != self.id {
            return false;
        }
        let Some(stage) = STAGES.iter().copied().find(|s| *s == stage) else {
            return false;
        };
        if self.marks.len() < MAX_MARKS {
            let ms = at.saturating_duration_since(self.started).as_millis();
            self.marks.push((stage, ms));
        }
        if let Some(detail) = detail.map(clean).filter(|d| !d.is_empty()) {
            self.detail = Some(detail);
        }
        if self.error.is_none() {
            if let Some(error) = error.map(clean).filter(|e| !e.is_empty()) {
                self.error = Some(format!("{stage}: {error}"));
            }
        }
        true
    }
    fn summary(&self) -> String {
        let marks = if self.marks.is_empty() {
            "the overlay page reported no stage".to_string()
        } else {
            let reached: Vec<String> = self
                .marks
                .iter()
                .map(|(stage, ms)| format!("{stage} +{ms} ms"))
                .collect();
            format!("page reached {}", reached.join(", "))
        };
        match &self.detail {
            Some(detail) => format!("{marks}; {detail}"),
            None => marks,
        }
    }
}
/// Page text is untrusted and goes into run.log: one bounded line, no control characters.
fn clean(text: &str) -> String {
    let line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_TEXT)
        .collect();
    line.trim().to_string()
}
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
    *PROGRESS.lock().unwrap() = Some(Progress::new(frame.id, Instant::now()));
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
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("dropzones.html".into()))
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
        .resizable(false);
    // Linux keys WebKit contexts by data directory, and its custom schemes (tauri:, ipc:) belong to
    // the context. Left unset, this overlay shares the default key with the incognito notch, whose
    // schemes were registered on a throwaway ephemeral context, so the page never loads. One stable,
    // app-owned, non-incognito context is reused by every carry for the life of the app.
    #[cfg(target_os = "linux")]
    let builder = builder.data_directory(web_context_dir());
    let w = builder.build().map_err(|e| e.to_string())?;
    pin(&w, screen)?;
    w.set_ignore_cursor_events(true)
        .map_err(|e| e.to_string())?;
    Ok(())
}
/// The carry overlay's own WebKit data directory: fixed (never per carry) and inside the app's cache.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn web_context_dir() -> std::path::PathBuf {
    crate::config::cache_dir().join("webview").join(LABEL)
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
/// The overlay page's progress or failure before its first-frame ACK. Never an ACK itself.
#[tauri::command]
pub fn carry_overlay_report(id: u64, stage: String, detail: Option<String>, error: Option<String>) {
    if let Some(progress) = PROGRESS.lock().unwrap().as_mut() {
        progress.record(
            id,
            &stage,
            detail.as_deref(),
            error.as_deref(),
            Instant::now(),
        );
    }
}
/// A failure the overlay page reported for this scene, with the stage it failed in.
pub fn failure(id: u64) -> Option<String> {
    PROGRESS
        .lock()
        .unwrap()
        .as_ref()
        .filter(|p| p.id == id)
        .and_then(|p| p.error.clone())
}
/// Where the overlay page got to for this scene, in milliseconds since `show`.
pub fn progress(id: u64) -> String {
    PROGRESS
        .lock()
        .unwrap()
        .as_ref()
        .filter(|p| p.id == id)
        .map(Progress::summary)
        .unwrap_or_else(|| "no overlay scene was shown".into())
}
#[tauri::command]
pub fn carry_revealed(id: u64) {
    REVEALED.store(id, SeqCst);
}
pub fn hide(app: &AppHandle) {
    *CURRENT.lock().unwrap() = None;
    *PROGRESS.lock().unwrap() = None;
    READY.store(0, SeqCst);
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
}
#[tauri::command]
pub fn get_zones() -> Option<Scene> {
    CURRENT.lock().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::{clean, web_context_dir, Progress, MAX_MARKS, MAX_TEXT};
    use std::time::{Duration, Instant};

    #[test]
    fn the_overlay_web_context_is_one_stable_app_owned_directory() {
        let dir = web_context_dir();
        // Same key every carry: one retained context, schemes registered once, nothing leaked.
        assert_eq!(dir, web_context_dir());
        assert!(dir.starts_with(crate::config::cache_dir()));
        assert!(dir.ends_with("webview/dropzones"));
        // Its own context, never the settings window's.
        assert_ne!(dir, crate::config::cache_dir().join("webview").join("settings"));
    }

    #[test]
    fn stages_are_timed_from_show_and_summarised_in_order() {
        let start = Instant::now();
        let mut p = Progress::new(7, start);
        assert!(p.record(
            0,
            "script",
            Some("viewport 1920x1080"),
            None,
            start + Duration::from_millis(5)
        ));
        assert!(p.record(7, "zones", None, None, start + Duration::from_millis(40)));
        assert!(p.record(
            7,
            "drawn",
            Some("no animation frame after 1000 ms; visibility=hidden"),
            None,
            start + Duration::from_millis(90)
        ));
        assert_eq!(
            p.summary(),
            "page reached script +5 ms, zones +40 ms, drawn +90 ms; no animation frame after 1000 ms; visibility=hidden"
        );
        assert_eq!(p.error, None);
    }

    #[test]
    fn another_scene_or_unknown_stage_is_ignored() {
        let start = Instant::now();
        let mut p = Progress::new(7, start);
        assert!(!p.record(8, "zones", None, Some("stale"), start));
        // "ready" is not a stage: only carry_overlay_ready acknowledges a painted frame.
        assert!(!p.record(7, "ready", None, None, start));
        assert!(!p.record(7, "frame", None, None, start));
        assert!(p.marks.is_empty() && p.error.is_none());
        assert_eq!(p.summary(), "the overlay page reported no stage");
    }

    #[test]
    fn the_first_page_failure_is_kept_with_its_stage() {
        let start = Instant::now();
        let mut p = Progress::new(7, start);
        p.record(7, "cells", None, Some("cell 2 failed to load"), start);
        p.record(7, "drawn", None, Some("later error"), start);
        assert_eq!(p.error.as_deref(), Some("cells: cell 2 failed to load"));
        // Empty or whitespace text is not a failure.
        let mut q = Progress::new(9, start);
        q.record(9, "zones", Some("  "), Some(" \n "), start);
        assert_eq!((q.error, q.detail), (None, None));
    }

    #[test]
    fn page_text_and_marks_are_bounded() {
        let start = Instant::now();
        let mut p = Progress::new(1, start);
        for _ in 0..50 {
            p.record(1, "drawn", None, None, start);
        }
        assert_eq!(p.marks.len(), MAX_MARKS);
        let long = "x".repeat(10_000);
        assert_eq!(clean(&long).chars().count(), MAX_TEXT);
        assert_eq!(clean("a\nb\r\tc\u{1b}[31m"), "a b  c [31m");
        assert!(!clean("line\nbreak").contains('\n'));
    }
}
