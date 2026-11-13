//! Presentation settings shared by the Windows notch, tray and settings window.
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::collections::BTreeMap;
use tauri::{AppHandle, Emitter, Manager};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParitySettings {
    pub weekly_headline: bool,
    pub claude_daily_pace: bool,
    pub show_usage_pace: bool,
    pub asks_provider_on_look: bool,
    pub show_codex_extra_limits: bool,
    pub weekly_ring_dashed: bool,
    pub weekly_reading: bool,
    pub folds_for_fullscreen: bool,
    pub uses_custom_scale: bool,
    pub custom_scale: f64,
    pub account_nicknames: BTreeMap<String, String>,
    pub account_order: Vec<String>,
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
            weekly_headline:false, claude_daily_pace:false, show_usage_pace:false, asks_provider_on_look:false,
            show_codex_extra_limits:true, weekly_ring_dashed:false, weekly_reading:false, folds_for_fullscreen:true,
            uses_custom_scale:false, custom_scale:1.0, account_nicknames:BTreeMap::new(), account_order:Vec::new(),
            shows_remaining: false, ring_order: "manual".into(), hide_depleted: false,
            shut_rings_when_spent: true, shows_notch_readings: true,
            menu_bar_reading: "both".into(), menu_bar_click_action: "menu".into(),
            menu_bar_activity_style: "pulse".into(), shows_limits_in_tray: false,
            tray_providers: Vec::new(),
        }
    }
}

impl ParitySettings {
    /// A source-level move carries its accounts together while retaining their internal order.
    /// Unknown account IDs survive reconciliation for when that source returns.
    pub fn reorder_provider_groups(&mut self, providers: &[String]) {
        self.account_order.sort_by_key(|id| {
            let base = if id.starts_with("claude@") { "claude" } else { id.as_str() };
            providers.iter().position(|p| p == base).unwrap_or(usize::MAX)
        });
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.custom_scale.is_finite() || !(0.5..=1.5).contains(&self.custom_scale) { return Err("Choose a custom size between 50% and 150%.".into()); }
        // Stable account IDs preserve profile names verbatim, including Unicode and punctuation.
        let valid_id=|id:&str|!id.trim().is_empty()&&id.len()<=1024&&!id.chars().any(char::is_control);
        if self.account_nicknames.len()>100 || self.account_nicknames.iter().any(|(id,name)|!valid_id(id)||name.chars().count()>80||name.chars().any(char::is_control)) {
            return Err("Account nicknames must be at most 80 characters without control characters.".into());
        }
        if self.account_order.len()>100 || self.account_order.iter().any(|id|!valid_id(id)) { return Err("Invalid account order.".into()); }
        let mut order=self.account_order.clone();order.sort();order.dedup();if order.len()!=self.account_order.len(){return Err("An account was ordered twice.".into());}

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
    let scale_changed = {
        let state = app.state::<crate::AppState>();
        let mut cfg = state.cfg.lock().unwrap();
        let scale_changed = cfg.parity.uses_custom_scale != settings.uses_custom_scale || cfg.parity.custom_scale != settings.custom_scale;
        let mut next = cfg.clone();
        next.parity = settings.clone();
        crate::config::save_checked(&next).map_err(|_| "Could not save display preferences.".to_string())?;
        *cfg = next;
        scale_changed
    };
    let _ = app.emit("parity_settings", &settings);
    if scale_changed { crate::place_notch(&app); }
    crate::tray::configure_click(&app);
    crate::tray::refresh_menu(&app);
    Ok(settings)
}

/// Mirrors native DailyPace: seven cumulative daily shares anchored to the weekly reset.
/// This window has no duration: measuring the pace of a pace would invent a cycle.
pub fn daily_pace(weekly: &crate::usage::LimitWindow, now: u64) -> Option<crate::usage::LimitWindow> {
    if weekly.count.is_some() || !weekly.used.is_finite() || weekly.used < 0.0 { return None; }
    let reset = i128::from(weekly.resets_at?);
    const DAY: i128 = 86_400_000;
    let start = reset - 7 * DAY;
    let elapsed = (i128::from(now) - start).clamp(0, 7 * DAY);
    let day = (elapsed / DAY).min(6);
    let next = if day == 6 { reset } else { start + (day + 1) * DAY };
    Some(crate::usage::LimitWindow { id: "daily_pace".into(), label: "Daily pace".into(),
        used: weekly.used / ((day + 1) as f64 / 7.0), resets_at: u64::try_from(next).ok(), ..Default::default() })
}

pub fn apply_daily_pace(id: &str, snapshot: &mut crate::usage::UsageSnapshot, enabled: bool, now: u64) {
    if !enabled || !(id == "claude" || id.starts_with("claude@")) || snapshot.windows.iter().any(|w| w.id == "daily_pace") { return; }
    if let Some(daily) = snapshot.windows.iter().find(|w| matches!(w.id.as_str(), "weekly_all" | "seven_day" | "weekly"))
        .and_then(|weekly| daily_pace(weekly, now)) { snapshot.windows.insert(0, daily); }
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
    if crate::platform::preview() || !settings(&app).asks_provider_on_look { return false; }
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

#[tauri::command]
pub fn open_dashboard(app: AppHandle) { open_dashboard_window(&app); }

pub fn open_dashboard_window(app: &AppHandle) {
    refresh_on_look(app.clone());
    let handle = app.clone();
    std::thread::spawn(move || {
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            if let Some(window) = app.get_webview_window("dashboard") {
                let _ = window.unminimize(); let _ = window.show(); let _ = window.set_focus();
                return;
            }
            let size = app.get_webview_window("notch")
                .and_then(|w| w.current_monitor().ok().flatten())
                .map(|m| ((m.size().width as f64 / m.scale_factor() - 64.0).clamp(820.0, 1080.0),
                    (m.size().height as f64 / m.scale_factor() - 100.0).clamp(600.0, 760.0)))
                .unwrap_or((1080.0, 760.0));
            let builder = tauri::WebviewWindowBuilder::new(&app, "dashboard", tauri::WebviewUrl::App("dashboard.html".into()))
                .incognito(crate::platform::preview())
                .title("Mātrā Dashboard").inner_size(size.0, size.1)
                .min_inner_size(820.0, 600.0).center();
            #[cfg(not(target_os = "macos"))]
            let builder = builder.data_directory(crate::config::cache_dir().join("webview/dashboard"));
            match builder.build() {
                Ok(window) => { let _ = window.set_focus(); }
                Err(_) => crate::applog("Could not open Dashboard."),
            }
        });
    });
}

pub fn open_activity_window(app: &AppHandle) {
    refresh_on_look(app.clone());
    let handle = app.clone();
    std::thread::spawn(move || {
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            if let Some(window) = app.get_webview_window("activity") {
                let _ = window.unminimize(); let _ = window.show(); let _ = window.set_focus();
                return;
            }
            let builder = tauri::WebviewWindowBuilder::new(&app, "activity", tauri::WebviewUrl::App("activity.html".into()))
                .incognito(crate::platform::preview())
                .title("Mātrā · Project Activity").inner_size(1000.0, 720.0)
                .min_inner_size(760.0, 520.0).center();
            #[cfg(not(target_os = "macos"))]
            let builder = builder.data_directory(crate::config::cache_dir().join("webview/activity"));
            match builder.build() {
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
    fn daily_pace_matches_the_shared_native_vectors_and_never_invents_unknown_use() {
        let vectors: serde_json::Value = serde_json::from_str(include_str!("../fixtures/presentation.json")).unwrap();
        for case in vectors["daily_pace"].as_array().unwrap() {
            let w = &case["weekly"];
            let weekly = crate::usage::LimitWindow { id:"seven_day".into(), used:w["used"].as_f64().unwrap_or(f64::NAN), resets_at:w["resets_at"].as_u64(), count:w["count"].as_i64(), ..Default::default() };
            let actual = daily_pace(&weekly, case["now"].as_u64().unwrap());
            if case["expected"].is_null() { assert!(actual.is_none(), "{}",case["name"]); }
            else { let actual=actual.unwrap();assert!((actual.used-case["expected"]["used"].as_f64().unwrap()).abs()<1e-12,"{}",case["name"]);assert_eq!(actual.resets_at,case["expected"]["resets_at"].as_u64());assert_eq!(actual.duration_ms,None); }
        }
    }
    #[test]
    fn display_preferences_validate_and_keep_unavailable_account_choices() {
        let mut settings=ParitySettings {account_nicknames:BTreeMap::from([("claude@工作/कार्यालय".into(),"Work".into())]),account_order:vec!["claude@工作/कार्यालय".into(),"custom:office".into()],..Default::default()};
        assert!(settings.validate().is_ok());
        assert_eq!(serde_json::from_str::<ParitySettings>(&serde_json::to_string(&settings).unwrap()).unwrap(),settings);
        let mut invalid=settings.clone();invalid.account_order.push("claude@bad\nidentity".into());assert!(invalid.validate().is_err());
        for scale in [0.5,1.0,1.5]{settings.custom_scale=scale;assert!(settings.validate().is_ok());}
        for scale in [0.49,1.51,f64::NAN]{settings.custom_scale=scale;assert!(settings.validate().is_err());}
        settings.custom_scale=1.0;settings.account_nicknames.insert("claude".into(),"bad\nname".into());assert!(settings.validate().is_err());
    }
    #[test]
    fn daily_pace_is_account_scoped_idempotent_and_optional() {
        let mut snapshot=crate::usage::UsageSnapshot {windows:vec![crate::usage::LimitWindow{id:"seven_day".into(),used:0.1,resets_at:Some(2_604_800_000),..Default::default()}],..Default::default()};
        for (id,on) in [("codex",true),("claude@work",false)]{apply_daily_pace(id,&mut snapshot,on,2_000_000_000);assert_eq!(snapshot.windows.len(),1);}
        apply_daily_pace("claude@work",&mut snapshot,true,2_000_000_000);apply_daily_pace("claude@work",&mut snapshot,true,2_000_000_000);assert_eq!(snapshot.windows.len(),2);assert_eq!(snapshot.windows[0].id,"daily_pace");
    }
    #[test]
    fn moving_a_provider_preserves_unicode_account_order_and_unavailable_ids() {
        let mut prefs=ParitySettings { account_order:vec!["claude@工作".into(),"codex".into(),"claude".into(),"missing".into()],..Default::default() };
        prefs.reorder_provider_groups(&["codex".into(),"claude".into()]);
        assert_eq!(prefs.account_order,["codex","claude@工作","claude","missing"]);
    }
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
