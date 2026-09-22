//! Explicit GitHub updates. OS HTTPS trust + GitHub SHA-256 verify transport and
//! integrity, NOT publisher code signing. Download and installation need separate clicks.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
const FEED: &str = "https://api.github.com/repos/panditfloki/matra/releases/latest";
const MAX_BYTES: u64 = 100 * 1024 * 1024;
#[derive(Clone, Serialize, Default)]
pub struct UpdateState {
    pub phase: String,
    pub available: Option<String>,
    pub checking: bool,
    pub installing: bool,
    pub downloaded: u64,
    pub total: u64,
    pub message: Option<String>,
}
#[derive(Clone, Deserialize, Debug)]
struct Release {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Clone, Deserialize, Debug)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
}
#[derive(Clone, Debug)]
struct Offer {
    version: String,
    url: String,
    size: u64,
    hash: String,
}
#[derive(Default)]
struct Store {
    ui: UpdateState,
    offer: Option<Offer>,
    ready: Option<PathBuf>,
}
static STORE: Mutex<Option<Store>> = Mutex::new(None);
static BUSY: AtomicBool = AtomicBool::new(false);
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::Release);
    }
}
fn acquire() -> Option<Guard> {
    BUSY.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .ok()
        .map(|_| Guard)
}
fn with_store<T>(f: impl FnOnce(&mut Store) -> T) -> T {
    let mut lock = STORE.lock().unwrap_or_else(|e| e.into_inner());
    f(lock.get_or_insert_with(Store::default))
}
fn emit(app: &AppHandle, ui: UpdateState) {
    with_store(|s| s.ui = ui.clone());
    let _ = app.emit("update_state", &ui);
}
fn fail(app: &AppHandle, message: &str) {
    let available = with_store(|s| {
        s.ready = None;
        s.offer.as_ref().map(|o| o.version.clone())
    });
    emit(
        app,
        UpdateState {
            phase: "error".into(),
            available,
            message: Some(message.into()),
            ..Default::default()
        },
    );
}
fn marker_path() -> PathBuf {
    crate::config::config_path().with_file_name("pending-update.json")
}
fn confirmation(current: &str, target: &str) -> Option<String> {
    let current = semver::Version::parse(current).ok()?;
    let target = semver::Version::parse(target).ok()?;
    (current >= target).then(|| format!("Updated to v{current}. Installation complete."))
}
#[tauri::command]
pub fn get_update_state() -> UpdateState {
    with_store(|s| {
        if s.ui.phase.is_empty() {
            s.ui.phase = "idle".into();
            s.ui.message = Some("Not checked yet.".into());
            if let Ok(raw) = std::fs::read_to_string(marker_path()) {
                if let Ok(target) = serde_json::from_str::<String>(&raw) {
                    if let Some(message) = confirmation(env!("CARGO_PKG_VERSION"), &target) {
                        s.ui.phase = "updated".into();
                        s.ui.message = Some(message);
                        let _ = std::fs::remove_file(marker_path());
                    } else {
                        s.ui.message = Some(format!(
                            "v{target} installation has not completed. Check again to retry."
                        ));
                    }
                }
            }
        }
        s.ui.clone()
    })
}
fn choose(release: Release, current: &str) -> Result<Option<Offer>, String> {
    if release.draft || release.prerelease {
        return Err("Latest release is not a stable public release.".into());
    }
    let tag = release.tag_name;
    let version = semver::Version::parse(tag.strip_prefix('v').ok_or("Invalid release tag.")?)
        .map_err(|_| "Invalid release version.")?;
    if !version.pre.is_empty() || !version.build.is_empty() {
        return Err("Invalid stable release version.".into());
    }
    if version <= semver::Version::parse(current).map_err(|_| "Invalid installed version.")? {
        return Ok(None);
    }
    let matches: Vec<_> = release
        .assets
        .into_iter()
        .filter(|a| a.name == "Matra-Setup.exe")
        .collect();
    if matches.len() != 1 {
        return Err("Release does not have exactly one Windows installer.".into());
    }
    let asset = &matches[0];
    let expected =
        format!("https://github.com/panditfloki/matra/releases/download/{tag}/Matra-Setup.exe");
    if asset.browser_download_url != expected || asset.size == 0 || asset.size > MAX_BYTES {
        return Err("Invalid installer URL or size.".into());
    }
    let hash = asset
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .ok_or("GitHub has not supplied an installer checksum.")?;
    if hash.len() != 64 || !hash.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err("Invalid installer checksum.".into());
    }
    Ok(Some(Offer {
        version: version.to_string(),
        url: expected,
        size: asset.size,
        hash: hash.to_lowercase(),
    }))
}
fn fetch(current: &str) -> Result<Option<Offer>, String> {
    let response = crate::http::builder().redirects(0).build().get(FEED).set("User-Agent", "Matra-updater")
        .set("Accept", "application/vnd.github+json").call()
        .map_err(|_| "Could not reach GitHub. Check your connection or retry later (GitHub may be rate-limiting).")?;
    let mut raw = Vec::new();
    response
        .into_reader()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| "Could not read GitHub response.")?;
    if raw.len() > 2 * 1024 * 1024 {
        return Err("GitHub response is too large.".into());
    }
    choose(
        serde_json::from_slice(&raw).map_err(|_| "GitHub returned invalid release metadata.")?,
        current,
    )
}
#[tauri::command]
pub fn check_for_update(app: AppHandle) {
    let Some(guard) = acquire() else { return };
    with_store(|s| {
        s.offer = None;
        s.ready = None;
    });
    emit(
        &app,
        UpdateState {
            phase: "checking".into(),
            checking: true,
            message: Some("Checking GitHub…".into()),
            ..Default::default()
        },
    );
    std::thread::spawn(move || {
        let _guard = guard;
        match fetch(env!("CARGO_PKG_VERSION")) {
            Ok(Some(offer)) => {
                let version = offer.version.clone();
                with_store(|s| s.offer = Some(offer));
                emit(
                    &app,
                    UpdateState {
                        phase: "available".into(),
                        available: Some(version.clone()),
                        message: Some(format!("v{version} is available.")),
                        ..Default::default()
                    },
                );
            }
            Ok(None) => emit(
                &app,
                UpdateState {
                    phase: "current".into(),
                    message: Some("You have the latest stable release.".into()),
                    ..Default::default()
                },
            ),
            Err(e) => fail(&app, &e),
        }
    });
}
fn allowed_redirect(value: &str) -> bool {
    url::Url::parse(value).ok().is_some_and(|u| {
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
            && matches!(
                u.host_str(),
                Some(
                    "github.com"
                        | "release-assets.githubusercontent.com"
                        | "objects.githubusercontent.com"
                )
            )
    })
}
fn response_for(url: &str) -> Result<ureq::Response, String> {
    let agent = crate::http::builder()
        .timeout(Duration::from_secs(120))
        .redirects(0)
        .build();
    let mut target = url.to_string();
    for _ in 0..5 {
        if !allowed_redirect(&target) {
            return Err("Installer redirected outside trusted GitHub download hosts.".into());
        }
        let response = agent
            .get(&target)
            .set("User-Agent", "Matra-updater")
            .call()
            .map_err(|_| "Installer download failed. Please retry.")?;
        if response.status() == 200 {
            return Ok(response);
        }
        if matches!(response.status(), 301 | 302 | 303 | 307 | 308) {
            target = response
                .header("Location")
                .ok_or("Invalid download redirect.")?
                .to_string();
        } else {
            return Err("Unexpected download response.".into());
        }
    }
    Err("Too many download redirects.".into())
}
fn copy_verified(
    mut reader: impl Read,
    mut writer: impl Write,
    offer: &Offer,
    mut progress: impl FnMut(u64),
) -> Result<(), String> {
    let mut sha = Sha256::new();
    let mut received = 0u64;
    let mut bytes = [0u8; 64 * 1024];
    let deadline = Instant::now() + Duration::from_secs(300);
    loop {
        if Instant::now() > deadline {
            return Err("Download timed out.".into());
        }
        let count = reader
            .read(&mut bytes)
            .map_err(|_| "Download interrupted. Please retry.")?;
        if count == 0 {
            break;
        }
        received += count as u64;
        if received > offer.size || received > MAX_BYTES {
            return Err("Installer exceeds its declared size.".into());
        }
        writer
            .write_all(&bytes[..count])
            .map_err(|_| "Could not save the installer. Check free disk space.")?;
        sha.update(&bytes[..count]);
        progress(received);
    }
    if received != offer.size || format!("{:x}", sha.finalize()) != offer.hash {
        return Err("Installer verification failed. Nothing was installed.".into());
    }
    writer
        .flush()
        .map_err(|_| "Could not finish saving installer.")?;
    Ok(())
}
fn verify_file(path: &std::path::Path, offer: &Offer) -> Result<(), String> {
    copy_verified(
        std::fs::File::open(path)
            .map_err(|_| "Downloaded installer is missing. Check again to retry.")?,
        std::io::sink(),
        offer,
        |_| {},
    )
}
#[tauri::command]
pub fn download_update(app: AppHandle) {
    let Some(guard) = acquire() else { return };
    let Some(offer) = with_store(|s| s.offer.clone()) else {
        return;
    };
    emit(
        &app,
        UpdateState {
            phase: "downloading".into(),
            available: Some(offer.version.clone()),
            total: offer.size,
            message: Some("Downloading GitHub installer…".into()),
            ..Default::default()
        },
    );
    std::thread::spawn(move || {
        let _guard = guard;
        let result: Result<PathBuf, String> = (|| {
            let directory = dirs::cache_dir()
                .ok_or("No user cache directory.")?
                .join("matra-notch")
                .join("updates")
                .join(&offer.version);
            std::fs::create_dir_all(&directory)
                .map_err(|_| "Cannot create update download directory.")?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| "Invalid system clock.")?
                .as_nanos();
            let path = directory.join(format!("Matra-Setup-{stamp}.exe"));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|_| "Cannot create installer file.")?;
            let mut last_percent = 101;
            let outcome = response_for(&offer.url).and_then(|r| {
                copy_verified(r.into_reader(), &mut file, &offer, |received| {
                    let percent = received * 100 / offer.size;
                    if percent != last_percent {
                        last_percent = percent;
                        emit(
                            &app,
                            UpdateState {
                                phase: "downloading".into(),
                                available: Some(offer.version.clone()),
                                downloaded: received,
                                total: offer.size,
                                ..Default::default()
                            },
                        );
                    }
                })
            });
            drop(file);
            if let Err(e) = outcome {
                let _ = std::fs::remove_file(&path);
                return Err(e);
            }
            Ok(path)
        })();
        match result {
            Ok(path) => {
                with_store(|s| s.ready = Some(path));
                emit(
                    &app,
                    UpdateState {
                        phase: "ready".into(),
                        available: Some(offer.version),
                        downloaded: offer.size,
                        total: offer.size,
                        message: Some("SHA-256 verified. Ready to install and restart.".into()),
                        ..Default::default()
                    },
                );
            }
            Err(e) => fail(&app, &e),
        }
    });
}
#[tauri::command]
pub fn install_update(app: AppHandle) {
    let Some(guard) = acquire() else { return };
    let (offer, path) = with_store(|s| (s.offer.clone(), s.ready.clone()));
    let (Some(offer), Some(path)) = (offer, path) else {
        return;
    };
    emit(
        &app,
        UpdateState {
            phase: "installing".into(),
            installing: true,
            available: Some(offer.version.clone()),
            message: Some("Opening Windows Setup. Mātrā will close and restart.".into()),
            ..Default::default()
        },
    );
    std::thread::spawn(move || {
        let _guard = guard;
        let result: Result<(), String> = (|| {
            verify_file(&path, &offer)?;
            let marker = marker_path();
            std::fs::create_dir_all(marker.parent().ok_or("Invalid update status path.")?)
                .map_err(|_| "Cannot save update status.")?;
            std::fs::write(
                &marker,
                serde_json::to_vec(&offer.version).map_err(|_| "Cannot encode update status.")?,
            )
            .map_err(|_| "Cannot save update status.")?;
            // Passive NSIS displays progress; /R restarts the app after installation.
            let mut child = std::process::Command::new(&path)
                .args(["/P", "/R"])
                .spawn()
                .map_err(|_| "Could not start Windows Setup. Please retry.")?;
            if !child
                .wait()
                .map_err(|_| "Could not read Windows Setup status.")?
                .success()
            {
                return Err("Windows Setup did not complete. Please retry.".into());
            }
            Ok(())
        })();
        match result {
            Err(e) => fail(&app, &e),
            Ok(()) => emit(
                &app,
                UpdateState {
                    phase: "restart_pending".into(),
                    message: Some(
                        "Setup finished. Open Mātrā from Start to confirm the installed version."
                            .into(),
                    ),
                    ..Default::default()
                },
            ),
        }
        // Only the newly launched version can confirm success. Setup normally stops this process.
    });
}
pub fn check_on_launch(_app: &AppHandle) { /* Checks are explicit; preserve update confirmation. */
}

#[cfg(test)]
mod tests {
    use super::*;
    fn release() -> Release {
        Release {
            tag_name: "v1.9.0".into(),
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: "Matra-Setup.exe".into(),
                size: 3,
                digest: Some(format!("sha256:{:x}", Sha256::digest(b"abc"))),
                browser_download_url:
                    "https://github.com/panditfloki/matra/releases/download/v1.9.0/Matra-Setup.exe"
                        .into(),
            }],
        }
    }
    #[test]
    fn semantic_versions_and_no_downgrades() {
        assert!(choose(release(), "1.8.1").unwrap().is_some());
        assert!(choose(release(), "1.9.0").unwrap().is_none());
        assert!(choose(release(), "1.10.0").unwrap().is_none());
    }
    #[test]
    fn refuses_missing_digest_and_foreign_assets() {
        let mut r = release();
        r.assets[0].digest = None;
        assert!(choose(r, "1.8.1").is_err());
        let mut r = release();
        r.assets[0].browser_download_url = "https://evil.example/Matra-Setup.exe".into();
        assert!(choose(r, "1.8.1").is_err());
        let mut r = release();
        r.assets[0].size = MAX_BYTES + 1;
        assert!(choose(r, "1.8.1").is_err());
        let mut r = release();
        r.prerelease = true;
        assert!(choose(r, "1.8.1").is_err());
        let mut r = release();
        r.assets.push(r.assets[0].clone());
        assert!(choose(r, "1.8.1").is_err());
    }
    #[test]
    fn verifies_actual_bytes() {
        let offer = choose(release(), "1.8.1").unwrap().unwrap();
        assert!(copy_verified(&b"abc"[..], Vec::new(), &offer, |_| {}).is_ok());
        for bytes in [&b"ab"[..], &b"abcd"[..], &b"bad"[..]] {
            assert!(copy_verified(bytes, Vec::new(), &offer, |_| {}).is_err());
        }
    }
    #[test]
    fn restricts_redirects() {
        assert!(allowed_redirect(
            "https://release-assets.githubusercontent.com/file?signature=example"
        ));
        for url in [
            "http://github.com/file",
            "https://github.com.evil.test/file",
            "https://user@github.com/file",
            "https://evil.test/file",
            "file:///tmp/setup.exe",
        ] {
            assert!(!allowed_redirect(url));
        }
    }
    #[test]
    fn confirms_only_completed_version_change() {
        assert!(confirmation("1.8.1", "1.8.2").is_none());
        assert!(confirmation("1.8.2", "1.8.2").is_some());
    }
    #[test]
    #[ignore = "Live GitHub metadata/download integrity check; never executes installer"]
    fn live_github_download_does_not_install() {
        let offer = fetch("0.0.0").unwrap().unwrap();
        let response = response_for(&offer.url).unwrap();
        copy_verified(response.into_reader(), std::io::sink(), &offer, |_| {}).unwrap();
    }
}
