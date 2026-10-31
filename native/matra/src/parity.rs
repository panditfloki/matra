//! Presentation settings shared by the Windows notch, tray and settings window.
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParitySettings {
    pub shows_remaining: bool,
    pub ring_order: String,
    pub hide_depleted: bool,
    pub shut_rings_when_spent: bool,
    pub shows_notch_readings: bool,
    pub menu_bar_reading: String,
    pub menu_bar_click_action: String,
    pub menu_bar_activity_style: String,
    pub shows_limits_in_tray: bool,
    pub tray_providers: Vec<String>,
}

impl Default for ParitySettings {
    fn default() -> Self {
        Self {
            shows_remaining: false, ring_order: "manual".into(), hide_depleted: false,
            shut_rings_when_spent: false, shows_notch_readings: true,
            menu_bar_reading: "both".into(), menu_bar_click_action: "menu".into(),
            menu_bar_activity_style: "pulse".into(), shows_limits_in_tray: false,
            tray_providers: Vec::new(),
        }
    }
}

impl ParitySettings {
    pub fn validate(&self) -> Result<(), String> {
        if !["manual", "remaining", "burnDeficit"].contains(&self.ring_order.as_str()) {
            return Err("Choose a supported account order.".into());
        }
        if !["usage", "reset", "both"].contains(&self.menu_bar_reading.as_str()) {
            return Err("Choose usage, reset time or both.".into());
        }
        if !["menu", "refresh", "activity"].contains(&self.menu_bar_click_action.as_str()) {
            return Err("Choose a supported tray click action.".into());
        }
        if !["pulse", "wave", "none"].contains(&self.menu_bar_activity_style.as_str()) {
            return Err("Choose a supported working indicator.".into());
        }
        if self.tray_providers.len() > 20 || self.tray_providers.iter().any(|id| {
            id.is_empty() || id.len() > 128 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || b"-_: .".contains(&c))
        }) {
            return Err("Invalid tray provider selection.".into());
        }
        let mut unique = self.tray_providers.clone(); unique.sort(); unique.dedup();
        if unique.len() != self.tray_providers.len() { return Err("A tray provider was selected twice.".into()); }
        Ok(())
    }
}

#[tauri::command]
pub fn get_parity_settings(app: AppHandle) -> ParitySettings { settings(&app) }

pub fn settings(app: &AppHandle) -> ParitySettings {
    let value = app.state::<crate::AppState>().cfg.lock().unwrap().parity.clone();
    if value.validate().is_ok() { value } else { ParitySettings::default() }
}

#[tauri::command]
pub fn set_parity_settings(app: AppHandle, settings: ParitySettings) -> Result<ParitySettings, String> {
    settings.validate()?;
    {
        let state = app.state::<crate::AppState>();
        let mut cfg = state.cfg.lock().unwrap();
        let mut next = cfg.clone();
        next.parity = settings.clone();
        crate::config::save_checked(&next).map_err(|_| "Could not save display preferences.".to_string())?;
        *cfg = next;
    }
    let _ = app.emit("parity_settings", &settings);
    crate::tray::configure_click(&app);
    crate::tray::refresh_menu(&app);
    Ok(settings)
}

const LOOK_FLOOR_MS: u64 = 30_000;
static LAST_LOOK: Mutex<Option<u64>> = Mutex::new(None);

fn claim_look(last: &mut Option<u64>, now: u64) -> bool {
    if last.is_some_and(|at| now.saturating_sub(at) < LOOK_FLOOR_MS) { return false; }
    *last = Some(now);
    true
}

/// Looking never overrides a provider's own retry/backoff state.
#[tauri::command]
pub fn refresh_on_look(app: AppHandle) -> bool {
    if !claim_look(&mut LAST_LOOK.lock().unwrap(), crate::now_ms()) { return false; }
    let now = crate::now_ms();
    for id in crate::provider_ids() {
        let snapshot = crate::snapshot_of(&app, &id);
        if snapshot.backoff_until > now || now.saturating_sub(snapshot.fetched_at) < LOOK_FLOOR_MS { continue; }
        if id == "gemini" { crate::antigravity::request_hover_refresh(); }
        else { crate::refresh_provider(&app, &id); }
    }
    true
}

#[tauri::command]
pub fn open_activity(app: AppHandle) { open_activity_window(&app); }

pub fn open_activity_window(app: &AppHandle) {
    let handle = app.clone();
    std::thread::spawn(move || {
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            if let Some(window) = app.get_webview_window("activity") {
                let _ = window.unminimize(); let _ = window.show(); let _ = window.set_focus();
                return;
            }
            match tauri::WebviewWindowBuilder::new(&app, "activity", tauri::WebviewUrl::App("activity.html".into()))
                .title("Mātrā · Project Activity").inner_size(1000.0, 720.0)
                .min_inner_size(760.0, 520.0).center().build() {
                Ok(window) => { let _ = window.set_focus(); }
                Err(_) => crate::applog("Could not open Project Activity."),
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_settings_load_without_changing_presentation() {
        let old: ParitySettings = serde_json::from_str("{}").unwrap();
        assert_eq!(old, ParitySettings::default());
        assert!(!old.shows_limits_in_tray);
        let cfg: crate::config::Config = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.parity, old);
    }
    #[test]
    fn settings_round_trip_keeps_account_order_and_readings() {
        let settings = ParitySettings { shows_remaining: true, ring_order: "remaining".into(),
            tray_providers: vec!["codex".into(), "custom:work".into()], ..Default::default() };
        assert!(settings.validate().is_ok());
        assert_eq!(serde_json::from_str::<ParitySettings>(&serde_json::to_string(&settings).unwrap()).unwrap(), settings);
    }
    #[test]
    fn invalid_settings_do_not_silently_change_a_saved_choice() {
        let mut value = ParitySettings::default();
        value.ring_order = "random".into(); assert!(value.validate().is_err());
        value.ring_order = "manual".into();
        value.tray_providers = vec!["claude".into(), "claude".into()]; assert!(value.validate().is_err());
    }
    #[test]
    fn opening_repeatedly_respects_freshness_floor() {
        let mut at = None;
        assert!(claim_look(&mut at, 100));
        assert!(!claim_look(&mut at, 101));
        assert!(!claim_look(&mut at, 30_099));
        assert!(claim_look(&mut at, 30_100));
    }
}
