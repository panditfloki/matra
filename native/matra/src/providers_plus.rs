//! Providers added for Windows parity with the Mac app: Grok Bot, Higgsfield, DeepSeek, Ollama, LM Studio,
//! oMLX, llama.cpp and custom endpoints, plus the optional OpenCode bridge. Self-contained: it does not
//! change `Config`, `main` or the usage types. Every reader is off until the user enables it.
//!
//! This file is only the Tauri surface. The logic, and its tests, live in the submodules.
pub mod bridge;
pub mod custom;
pub mod deepseek;
pub mod engine;
pub mod grokbot;
pub mod fixed;
pub mod higgsfield;
pub mod local;
pub mod metrics;
pub mod model;
pub mod net;
pub mod opencode_auth;
pub mod settings;
pub mod vault;

use custom::{CustomEndpoint, Detection, Mapping, Preset, TestResult};
use engine::{Engine, Env, ProviderInfo, Urls};
use model::ParitySnapshot;
use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter};

static ENGINE: OnceLock<Arc<Engine>> = OnceLock::new();

fn engine() -> &'static Arc<Engine> {
    ENGINE.get_or_init(|| {
        let settings_dir = crate::config::config_path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        Arc::new(Engine::new(Env {
            settings_dir,
            home: if crate::platform::preview() { None } else { dirs::home_dir() },
            appdata: if crate::platform::preview() { None } else { dirs::config_dir() },
            local_appdata: if crate::platform::preview() { None } else { dirs::data_local_dir() },
            vars: Arc::new(|name| if crate::platform::preview() { None } else { std::env::var(name).ok() }),
            path_var: if crate::platform::preview() { None } else { std::env::var_os("PATH") },
            urls: Urls::default(),
        }))
    })
}

pub fn is_parity_id(id: &str) -> bool {
    engine::is_parity_id(id)
}

pub fn snapshot(id: &str) -> crate::usage::UsageSnapshot {
    engine().snapshot(id)
}

pub fn request_refresh(id: &str) -> bool {
    if crate::platform::preview() { return false; }
    engine().request_refresh(id)
}

pub fn enabled_ids() -> Vec<String> {
    engine().enabled_ids()
}

/// Starts the poll loop and event wiring. Reads nothing for a provider the user has not enabled.
pub fn start(app: AppHandle) {
    if crate::platform::preview() { return; }
    let engine = engine().clone();
    let emitter = app.clone();
    engine.set_notify(Arc::new(move || {
        let _ = emitter.emit("parity_usage", get_parity_usage());
        let _ = emitter.emit("provider_registry", get_provider_registry());
        crate::tray::refresh_menu(&emitter);
    }));
    std::thread::spawn(move || loop {
        engine.tick_and_spawn();
        std::thread::sleep(std::time::Duration::from_secs(1));
    });
}

#[tauri::command]
pub fn get_parity_usage() -> BTreeMap<String, ParitySnapshot> {
    engine().usage_map()
}

#[tauri::command]
pub fn get_provider_registry() -> Vec<ProviderInfo> {
    let mut registry = engine().registry();
    if crate::platform::preview() {
        for provider in &mut registry {
            provider.accepts_secret = false;
            provider.has_stored_secret = false;
            provider.has_env_secret = false;
            provider.guidance = "SYNTHETIC PREVIEW: provider discovery, credentials and network access are disabled.".into();
        }
    }
    registry
}

#[tauri::command]
pub fn set_parity_provider(id: String, on: bool) -> Result<ProviderInfo, String> {
    crate::platform::require_live("Provider access and integration changes")?;
    engine().set_enabled(&id, on)
}

#[tauri::command]
pub fn set_parity_endpoint(id: String, endpoint: Option<String>) -> Result<ProviderInfo, String> {
    crate::platform::require_live("Provider access and integration changes")?;
    engine().set_endpoint(&id, endpoint.as_deref())
}

#[tauri::command]
pub fn set_fixed_provider_options(id: String, options: settings::FixedOptions) -> Result<ProviderInfo, String> {
    engine().set_fixed_options(&id, options)
}

#[tauri::command]
pub fn set_provider_secret(id: String, secret: String) -> Result<ProviderInfo, String> {
    crate::platform::require_live("Provider access and integration changes")?;
    engine().set_secret(&id, &secret)
}

#[tauri::command]
pub fn clear_provider_secret(id: String) -> Result<ProviderInfo, String> {
    crate::platform::require_live("Provider access and integration changes")?;
    engine().clear_secret(&id)
}

#[tauri::command]
pub fn refresh_parity_provider(id: String) -> bool {
    if crate::platform::preview() { return false; }
    engine().request_refresh(&id)
}

#[tauri::command]
pub fn list_custom_endpoints() -> Vec<CustomEndpoint> {
    engine().list_custom()
}

#[tauri::command]
pub fn get_custom_presets() -> Vec<Preset> {
    engine().presets()
}

#[tauri::command]
pub fn save_custom_endpoint(endpoint: CustomEndpoint, secret: Option<String>) -> Result<CustomEndpoint, String> {
    crate::platform::require_live("Provider access and integration changes")?;
    engine().save_custom(endpoint, secret.as_deref())
}

#[tauri::command]
pub fn delete_custom_endpoint(id: String) -> Result<(), String> {
    crate::platform::require_live("Provider access and integration changes")?;
    engine().delete_custom(&id)
}

// The network-touching commands below are async so a slow endpoint cannot stall the UI thread.

#[tauri::command]
pub async fn test_custom_endpoint(endpoint: CustomEndpoint, secret: Option<String>) -> Result<TestResult, String> {
    crate::platform::require_live("Provider access and integration changes")?;
    let run = move || engine().test_custom(&endpoint, secret.as_deref());
    tauri::async_runtime::spawn_blocking(run).await.map_err(|_| "The test could not finish.".to_string())
}

#[tauri::command]
pub async fn detect_custom_usage(endpoint: CustomEndpoint, secret: Option<String>) -> Result<Detection, String> {
    crate::platform::require_live("Provider access and integration changes")?;
    let run = move || engine().detect_custom(&endpoint, secret.as_deref());
    tauri::async_runtime::spawn_blocking(run).await.map_err(|_| "Detection could not finish.".to_string())
}

#[tauri::command]
pub async fn scan_local_endpoints() -> Result<Vec<Preset>, String> {
    crate::platform::require_live("Provider access and integration changes")?;
    tauri::async_runtime::spawn_blocking(|| engine().scan()).await.map_err(|_| "The scan could not finish.".to_string())
}

#[tauri::command]
pub fn import_custom_usage_mapping(base: String, text: String) -> Result<Mapping, String> {
    engine().import_mapping(&base, &text)
}

#[tauri::command]
pub fn get_opencode_bridge_status() -> Result<bridge::Status, String> {
    crate::platform::require_live("Provider access and integration changes")?;
    engine().bridge_status()
}

#[tauri::command]
pub fn install_opencode_bridge() -> Result<bridge::Status, String> {
    crate::platform::require_integration("Installing the OpenCode bridge")?;
    engine().bridge_install()
}

#[tauri::command]
pub fn remove_opencode_bridge() -> Result<bridge::Status, String> {
    crate::platform::require_integration("Removing the OpenCode bridge")?;
    engine().bridge_remove()
}
