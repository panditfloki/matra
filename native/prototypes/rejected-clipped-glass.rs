//! REJECTED PROTOTYPE: testing showed a full-window backdrop outside the visible pill.
//! Not compiled or shipped. Native desktop blur, clipped to the visible surfaces rather than the overlay's
//! large transparent input window. No desktop capture, wallpaper copy or shader fake.
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri::window::{Effect, EffectsBuilder};
use tauri::utils::config::{Color, WindowEffectsConfig};

static LAST: Mutex<Option<(bool, Vec<[f64; 5]>)>> = Mutex::new(None);

#[tauri::command]
pub fn sync_glass(app: AppHandle, enabled: bool, shapes: Vec<[f64; 5]>) -> Result<bool, String> {
    let enabled = enabled && app.state::<crate::AppState>().cfg.lock().unwrap().theme == "glass";
    if shapes.len() > 8 || shapes.iter().flatten().any(|v| !v.is_finite() || v.abs() > 16384.0) {
        return Err("Invalid glass geometry".into());
    }
    let mut last = LAST.lock().unwrap();
    if last.as_ref() == Some(&(enabled, shapes.clone())) { return Ok(enabled); }
    let w = app.get_webview_window("notch").ok_or("No notch window")?;
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Gdi::{CreateRectRgn, CreateRoundRectRgn, CombineRgn, DeleteObject, SetWindowRgn, HRGN, RGN_OR};
        let hwnd = HWND(w.hwnd().map_err(|e| e.to_string())?.0);
        unsafe {
            if enabled {
                let region = CreateRectRgn(0, 0, 0, 0);
                if region.0.is_null() { return Err("Could not allocate glass region".into()); }
                for [x,y,width,height,radius] in &shapes {
                    if *width <= 0.0 || *height <= 0.0 { continue; }
                    let part = CreateRoundRectRgn(x.floor() as i32, y.floor() as i32,
                        (x+width).ceil() as i32+1, (y+height).ceil() as i32+1,
                        (radius*2.0).round() as i32, (radius*2.0).round() as i32);
                    if part.0.is_null() { let _=DeleteObject(region); return Err("Could not allocate glass shape".into()); }
                    let combined=CombineRgn(region, region, part, RGN_OR);
                    let _=DeleteObject(part);
                    if combined.0 == 0 { let _=DeleteObject(region); return Err("Could not combine glass shapes".into()); }
                }
                // Windows owns the region only after successful assignment.
                if SetWindowRgn(hwnd, region, true) == 0 {
                    let _=DeleteObject(region); return Err("Could not clip native glass".into());
                }
                if !last.as_ref().is_some_and(|v| v.0) {
                    w.set_theme(Some(tauri::Theme::Light)).map_err(|e| e.to_string())?;
                    w.set_effects(EffectsBuilder::new().effect(Effect::Acrylic).color(Color(250,248,246,32)).build())
                        .map_err(|e| { let _=SetWindowRgn(hwnd, HRGN::default(), true); e.to_string() })?;
                }
            } else {
                if last.as_ref().is_some_and(|v| v.0) {
                    w.set_effects(None::<WindowEffectsConfig>).map_err(|e| e.to_string())?;
                    w.set_theme(None).map_err(|e| e.to_string())?;
                }
                if SetWindowRgn(hwnd, HRGN::default(), true) == 0 { return Err("Could not clear glass clipping".into()); }
            }
        }
    }
    #[cfg(not(windows))]
    return Ok(false);
    *last=Some((enabled, shapes));
    Ok(enabled)
}
