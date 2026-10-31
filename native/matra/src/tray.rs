use crate::i18n::tr;
use crate::traymenu;
use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let mut builder = TrayIconBuilder::with_id("main");
    // Windows does not tint tray icons and the monochrome outline vanishes on a dark taskbar, so
    // the app's own mark is the icon. trayicon::app_mark explains why at length.
    if let Some(icon) = crate::trayicon::app_mark() {
        builder = builder.icon(icon);
    }
    builder
        .tooltip(concat!("dy/dx.f(Mātrā) v", env!("CARGO_PKG_VERSION")))
        .menu(&menu)
        .show_menu_on_left_click(crate::parity::settings(app).menu_bar_click_action == "menu")
        .on_menu_event(|app, ev| handle(app, ev.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button, button_state: MouseButtonState::Up, .. } = event {
                let app = tray.app_handle();
                if button == MouseButton::Left {
                    match crate::parity::settings(app).menu_bar_click_action.as_str() {
                        "refresh" => crate::refresh_all(app),
                        "activity" => crate::parity::open_activity_window(app),
                        _ => { crate::parity::refresh_on_look(app.clone()); }
                    }
                } else if button == MouseButton::Right {
                    crate::parity::refresh_on_look(app.clone());
                }
            }
        })
        .build(app)?;
    Ok(())
}

/// The readings themselves, as the Mac's menu bar shows them: a line per provider with its headline
/// figure, and under it one greyed line per limit window. The macOS menu is rebuilt as it opens;
/// Tauri has no such hook, so `refresh_menu` is called whenever a reading changes and once a minute
/// besides, which keeps "Resets in 12 min" honest.
/// Every line the menu would show, in order, with the id each carries. Kept apart from building the
/// menu so a refresh can tell whether anything visible changed before it swaps the menu out.
fn menu_lines(app: &AppHandle, lang: &str) -> Vec<(String, String, bool)> {
    let now = crate::now_ms();
    let mut lines = Vec::new();
    let prefs = crate::parity::settings(app);
    for id in ordered_providers(app, &prefs) {
        let snap = crate::snapshot_of(app, &id);
        if snap.status == "absent" {
            continue;
        }
        let head = traymenu::header(
            crate::provider_label(&id),
            crate::ring_fraction(app, &id).map(|used| if prefs.shows_remaining { 1.0 - used } else { used }),
            traymenu::stale_since(&snap, now),
            now,
            lang,
        );
        // Clicking a provider re-reads that one, as on the Mac.
        let head = if prefs.shows_remaining { format!("{head} · remaining") } else { head };
        lines.push((format!("refresh:{id}"), head, true));
        for (n, line) in traymenu::provider_lines(&snap, now, lang).iter().enumerate() {
            // Windows does not indent submenu-less items, so the indent is in the text.
            lines.push((format!("line:{id}:{n}"), format!("    {line}"), false));
        }
    }
    lines
}

pub fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let lang = language(app);
    build_menu_from(app, &lang, &menu_lines(app, &lang))
}

fn build_menu_from(app: &AppHandle, lang: &str, lines: &[(String, String, bool)]) -> tauri::Result<Menu<Wry>> {
    let lang = lang.to_string();
    let mut items: Vec<tauri::menu::MenuItem<Wry>> = Vec::new();
    for (id, text, enabled) in lines {
        items.push(MenuItemBuilder::with_id(id.clone(), text.clone()).enabled(*enabled).build(app)?);
    }
    if items.is_empty() {
        items.push(
            MenuItemBuilder::with_id("waiting", tr(&lang, "waiting"))
                .enabled(false)
                .build(app)?,
        );
    }
    let refresh = MenuItemBuilder::with_id("refresh", tr(&lang, "refresh_all")).build(app)?;
    let settings = MenuItemBuilder::with_id("settings", tr(&lang, "settings")).build(app)?;
    let activity = MenuItemBuilder::with_id("activity", "Project Activity…").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", tr(&lang, "quit_app")).build(app)?;
    let mut menu = MenuBuilder::new(app);
    for item in &items {
        menu = menu.item(item);
    }
    menu.separator()
        .item(&refresh)
        .item(&activity)
        .item(&settings)
        .separator()
        .item(&quit)
        .build()
}

/// The language the menu speaks, already resolved: `traymenu` picks its wording by code and has no
/// "auto" of its own.
pub(crate) fn language(app: &AppHandle) -> String {
    let st = app.state::<crate::AppState>();
    let raw = st.cfg.lock().unwrap().lang.clone();
    if raw == "auto" {
        crate::i18n::resolve_auto().to_string()
    } else {
        raw
    }
}

/// The hover text: the same figures the menu opens with, for when the menu is not open.
fn tooltip(app: &AppHandle) -> String {
    let mut parts: Vec<String> = Vec::new();
    let prefs = crate::parity::settings(app);
    for id in ordered_providers(app, &prefs) {
        if crate::snapshot_of(app, &id).status == "absent" {
            continue;
        }
        let value = crate::ring_fraction(app, &id)
            .map(|f| format!("{}% {}", traymenu::pct(if prefs.shows_remaining { 1.0 - f } else { f }),
                            if prefs.shows_remaining { "left" } else { "used" }))
            .unwrap_or_else(|| "—".into());
        parts.push(format!("{} {value}", crate::provider_label(&id)));
    }
    if parts.is_empty() {
        concat!("dy/dx.f(Mātrā) v", env!("CARGO_PKG_VERSION")).to_string()
    } else {
        format!("dy/dx.f(Mātrā): {}", parts.join(" · "))
    }
}

/// Rebuilds the tray menu, ALWAYS on the main thread.
///
/// A menu is a Windows UI object. Building one or swapping it in from another thread leaves the
/// tray holding a menu that never opens again — and since changing the language is what triggers a
/// rebuild, the user is then locked out of the only place they could change it back. The tray's own
/// click handlers already run on the main thread, but the readings poller and the settings window
/// do not, so the hop is done here once rather than being remembered at every call site.
/// What the menu last showed, so an unchanged refresh leaves it alone.
static SHOWN: std::sync::Mutex<Option<(String, Vec<(String, String, bool)>)>> = std::sync::Mutex::new(None);

/// Swaps the menu only when a line of it would read differently. `set_menu` replaces the menu the
/// user may have open this moment — the refresh runs on the main thread, which the open popup's
/// message loop still serves — so the minute tick used to close it under the pointer even when
/// "Resets in 12 min" still said 12 min.
pub fn refresh_menu(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        update_reading_icons(&handle);
        if let Some(tray) = handle.tray_by_id("main") {
            let lang = language(&handle);
            let lines = menu_lines(&handle, &lang);
            let key = (lang.clone(), lines.clone());
            if SHOWN.lock().unwrap().as_ref() == Some(&key) {
                // A tooltip can change without a line changing, and setting it closes nothing.
                let _ = tray.set_tooltip(Some(&tooltip(&handle)));
                return;
            }
            match build_menu_from(&handle, &lang, &lines) {
                Ok(menu) => {
                    let _ = tray.set_menu(Some(menu));
                    for id in READING_ICONS.lock().unwrap().iter() {
                        if let Some(reading) = handle.tray_by_id(&format!("reading:{id}")) {
                            if let Ok(menu) = build_menu_from(&handle, &lang, &lines) { let _ = reading.set_menu(Some(menu)); }
                        }
                    }
                    let _ = tray.set_tooltip(Some(&tooltip(&handle)));
                    *SHOWN.lock().unwrap() = Some(key);
                }
                Err(e) => crate::applog(&format!("tray menu: {e}")),
            }
        }
    });
}

/// Provider rows carry `refresh:<id>`; everything else the menu offers is one of the four fixed
/// items. Settings, language, hooks and the rest arrive as commands from the settings window.
fn handle(app: &AppHandle, id: &str) {
    if let Some(provider) = id.strip_prefix("refresh:") {
        crate::refresh_provider(app, provider);
        return;
    }
    match id {
        "refresh" => crate::refresh_all(app),
        "settings" => crate::settings_window::open(app),
        "activity" => crate::parity::open_activity_window(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

pub fn configure_click(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_show_menu_on_left_click(crate::parity::settings(app).menu_bar_click_action == "menu");
    }
}

fn ordered_providers(app: &AppHandle, prefs: &crate::parity::ParitySettings) -> Vec<String> {
    let slots = app.state::<crate::AppState>().cfg.lock().unwrap().notch_slots.clone();
    let mut ids = crate::provider_ids();
    ids.sort_by_key(|id| slots.iter().position(|slot| slot.provider == *id).unwrap_or(usize::MAX));
    if prefs.hide_depleted {
        ids.retain(|id| crate::snapshot_of(app, id).status != "ok"
            || crate::ring_fraction(app, id).is_none_or(|used| used < 1.0));
    }
    if prefs.ring_order != "manual" {
        let now = crate::now_ms();
        let key = |id: &str| {
            let snap = crate::snapshot_of(app, id);
            quota_order(&snap, crate::ring_fraction(app, id), &prefs.ring_order, now)
        };
        // Stable ties preserve the user's manual arrangement.
        ids.sort_by(|a, b| {
            let (ta, ba, ra) = key(a); let (tb, bb, rb) = key(b);
            ta.cmp(&tb).then_with(|| bb.total_cmp(&ba)).then_with(|| rb.total_cmp(&ra))
        });
    }
    ids
}

/// A reset clock is comparable only when the window declares its cycle.
fn cycle_ms(window: &crate::usage::LimitWindow) -> Option<f64> {
    let id = window.id.split('@').next().unwrap_or("");
    let label = window.label.to_ascii_lowercase();
    if id.starts_with("seven_day") || id.starts_with("weekly") || id.ends_with("-weekly") || label.contains("week") {
        Some(7.0 * 86_400_000.0)
    } else if id == "monthly" || label.contains("month") {
        Some(30.0 * 86_400_000.0)
    } else if id == "session" || id == "five_hour" || id == "5h" || id.ends_with("-5h") || label.contains("five-hour") {
        Some(5.0 * 3_600_000.0)
    } else { None }
}

fn quota_order(snapshot: &crate::usage::UsageSnapshot, used: Option<f64>, order: &str, now: u64) -> (u8, f64, f64) {
    let unknown = if order == "burnDeficit" { 4 } else { 3 };
    let Some(used) = used.filter(|v| v.is_finite() && *v >= 0.0) else { return (unknown, 0.0, 0.0) };
    if snapshot.status != "ok" { return (unknown, 0.0, 0.0); }
    let left = 1.0 - used;
    let spent = snapshot.windows.iter().any(|w| w.count.is_none() && w.used.is_finite() && w.used >= 1.0);
    if order != "burnDeficit" {
        return (if used >= 1.0 { 2 } else if spent { 1 } else { 0 }, 0.0, left);
    }
    let burn = snapshot.windows.iter().filter_map(|w| {
        if w.count.is_some() || !w.used.is_finite() || w.used < 0.0 { return None; }
        let remaining = w.resets_at?.checked_sub(now)?;
        if remaining == 0 { return None; }
        Some(1.0 - (remaining as f64 / cycle_ms(w)?).min(1.0) - w.used.min(1.0))
    }).max_by(f64::total_cmp);
    let tier = if used >= 1.0 { 3 } else if spent { 2 } else if burn.is_some() { 0 } else { 1 };
    (tier, burn.unwrap_or(f64::NEG_INFINITY), left)
}

static READING_ICONS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn update_reading_icons(app: &AppHandle) {
    let prefs = crate::parity::settings(app);
    let visible = app.state::<crate::AppState>().cfg.lock().unwrap().tray_visible;
    let ids: Vec<String> = if prefs.shows_limits_in_tray && visible {
        ordered_providers(app, &prefs).into_iter()
            .filter(|id| prefs.tray_providers.is_empty() || prefs.tray_providers.contains(id))
            .filter(|id| crate::snapshot_of(app, id).status != "absent").collect()
    } else { Vec::new() };
    let mut previous = READING_ICONS.lock().unwrap();
    for id in previous.iter().filter(|id| !ids.contains(id)) {
        drop(app.remove_tray_by_id(&format!("reading:{id}")));
    }
    for id in &ids {
        let snapshot = crate::snapshot_of(app, id);
        let (limit, model, colors) = {
            let state = app.state::<crate::AppState>(); let cfg = state.cfg.lock().unwrap();
            (cfg.antigravity_limit.clone(), cfg.antigravity_model.clone(), cfg.quota_colors.clone())
        };
        let window = crate::ring_window(id, &snapshot.windows, &limit, &model);
        let used = crate::ring_fraction(app, id);
        let reset = window.and_then(|w| w.resets_at);
        let icon = crate::trayicon::reading(used, reset, crate::now_ms(), &prefs, &colors);
        let details = traymenu::provider_lines(&snapshot, crate::now_ms(), &language(app)).join(" · ");
        let title = format!("{} · {}", crate::provider_label(id), details);
        let tray_id = format!("reading:{id}");
        if let Some(tray) = app.tray_by_id(&tray_id) {
            let _ = tray.set_icon(Some(icon)); let _ = tray.set_tooltip(Some(&title));
            let _ = tray.set_show_menu_on_left_click(prefs.menu_bar_click_action == "menu");
        } else {
            let menu = match build_menu(app) { Ok(menu) => menu, Err(_) => continue };
            let action_id = id.clone();
            let _ = TrayIconBuilder::with_id(tray_id).icon(icon).tooltip(title).menu(&menu)
                .show_menu_on_left_click(prefs.menu_bar_click_action == "menu")
                .on_menu_event(|app, event| handle(app, event.id().as_ref()))
                .on_tray_icon_event(move |tray, event| {
                    if matches!(event, TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }) {
                        let app = tray.app_handle();
                        match crate::parity::settings(app).menu_bar_click_action.as_str() {
                            "refresh" => { crate::refresh_provider(app, &action_id); }
                            "activity" => crate::parity::open_activity_window(app),
                            _ => { crate::parity::refresh_on_look(app.clone()); }
                        }
                    }
                }).build(app);
        }
    }
    *previous = ids;
}

pub fn start_activity_badges(app: AppHandle) {
    std::thread::spawn(move || {
        let mut previous_busy = false;
        let mut completed_until = 0;
        let mut last = (String::new(), 0u8);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(750));
            let state = app.state::<crate::AppState>();
            let snapshot = state.store.lock().unwrap().snapshot("en", "en", true, false);
            let activity = state.activity.lock().unwrap().clone();
            let waiting = snapshot.counts.get("attention").copied().unwrap_or(0) > 0
                || activity.iter().any(|a| a.state == "waiting");
            let busy = snapshot.counts.get("running").copied().unwrap_or(0) > 0
                || activity.iter().any(|a| a.state == "busy");
            let now = crate::now_ms();
            if previous_busy && !busy {
                completed_until = now + 3000;
                crate::parity::refresh_on_look(app.clone());
            }
            previous_busy = busy;
            let prefs = crate::parity::settings(&app);
            let motion = state.cfg.lock().unwrap().notch_motion;
            let badge = if waiting { "waiting" } else if now < completed_until { "done" }
                else if busy && prefs.menu_bar_activity_style != "none" { "busy" } else { "idle" };
            let phase = if busy && motion && prefs.menu_bar_activity_style != "none" { ((now / 750) % 2) as u8 } else { 0 };
            let key = (format!("{badge}:{}", prefs.menu_bar_activity_style), phase);
            if last == key { continue; }
            last = key;
            let badge = badge.to_string(); let handle = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Some(tray) = handle.tray_by_id("main") {
                    let _ = tray.set_icon(crate::trayicon::activity_mark(&badge, phase, &prefs.menu_bar_activity_style));
                }
            });
        }
    });
}

#[cfg(test)]
mod parity_tests {
    use super::*;
    fn sample(used: f64, reset: u64) -> crate::usage::UsageSnapshot {
        serde_json::from_value(serde_json::json!({"status":"ok", "windows":[
            {"id":"weekly", "label":"Weekly", "used":used, "resets_at":reset}
        ], "fetched_at":1, "note":"", "backoff_until":0})).unwrap()
    }
    #[test]
    fn expiring_allowance_precedes_fresh_larger_allowance() {
        let expiring = sample(0.3, 23 * 3_600_000);
        let fresh = sample(0.0, 6 * 86_400_000);
        assert!(quota_order(&expiring, Some(0.3), "burnDeficit", 0).1 > quota_order(&fresh, Some(0.0), "burnDeficit", 0).1);
        assert!(quota_order(&expiring, Some(0.3), "remaining", 0).2 < quota_order(&fresh, Some(0.0), "remaining", 0).2);
    }
    #[test]
    fn stale_counts_and_spent_limits_do_not_look_usable() {
        let mut snap = sample(1.0, 1000);
        assert_eq!(quota_order(&snap, Some(0.1), "remaining", 0).0, 1);
        snap.status = "stale".into();
        assert_eq!(quota_order(&snap, Some(0.1), "remaining", 0).0, 3);
        assert_eq!(quota_order(&snap, None, "burnDeficit", 0).0, 4);
    }
}
