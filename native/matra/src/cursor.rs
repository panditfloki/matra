//! Cursor usage adapter, implemented from the upstream Matra's documented behaviour.
//!
//! Data path (same trade-off as upstream: borrow the editor's own session):
//!   1. Credential: the editor keeps its sign-in in the global state database it inherited from
//!      VS Code, `%APPDATA%\Cursor\User\globalStorage\state.vscdb` (SQLite, table ItemTable(key,value)):
//!      `cursorAuth/accessToken` + `cursorAuth/stripeMembershipAuthId`, joined into the cookie
//!      `WorkosCursorSessionToken=<authId>::<token>`. Non-secret identity cache:
//!      `cursorAuth/cachedEmail`, `cursorAuth/stripeMembershipType` (only the plan is shown).
//!      Current access tokens have a `cursor-safe-storage:v1:` base64 envelope. On Windows,
//!      Electron uses `v10` + a 12-byte nonce + AES-256-GCM ciphertext and a 16-byte tag.
//!      Its DPAPI-protected key is read from the same Cursor root's `Local State` file.
//!      Unsupported or undecryptable envelopes never become request headers.
//!   2. Endpoint: `GET https://cursor.com/api/usage-summary` (Cookie + Accept: application/json, 15 s).
//!      Reply:
//!      ```text
//!      { billingCycleEnd, membershipType, isUnlimited,
//!        individualUsage: { plan: { totalPercentUsed, apiPercentUsed, used, limit, breakdown },
//!                           onDemand: { enabled, used, limit } } }
//!      ```
//!      Cursor meters a percentage of the allowance, not requests: the dashboard's
//!      "Included usage · N% used" is totalPercentUsed. On the free plan used/limit are always 0
//!      (the allowance arrives as breakdown.bonus), so reading used/limit would report 10 % as 0 %.
//!      0 is a reading, not a gap (upstream's lesson). "API usage" is listed separately when
//!      apiPercentUsed > 0; "On demand" when onDemand has a real limit.
//!
//! SQLite opening rule: `mode=ro` first (it sees the token the editor just rotated into the WAL),
//! then `immutable=1` (once the editor has exited and the -shm is gone, mode=ro fails to open; by
//! then the WAL has been checkpointed, so ignoring it costs nothing).
//! Read only, never written; token values never reach logs, events or the UI.

use crate::usage::{LimitWindow, UsageSnapshot};
use crate::AppState;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const ENDPOINT: &str = "https://cursor.com/api/usage-summary";
const POLL_SECS: u64 = 300;
const SAFE_STORAGE_NAMESPACE: &str = "cursor-safe-storage:";
const SAFE_STORAGE_V1: &str = "cursor-safe-storage:v1:";

static REFRESH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn request_refresh() {
    REFRESH.store(true, std::sync::atomic::Ordering::Relaxed);
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Windows: %APPDATA%\Cursor\User\globalStorage\state.vscdb (macOS: ~/Library/Application Support/Cursor/...)
pub fn store_url() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join("Cursor").join("User").join("globalStorage").join("state.vscdb"))
}

fn store_path() -> PathBuf {
    crate::config::config_path().with_file_name("cursor.json")
}

pub fn load_persisted() -> UsageSnapshot {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|t| serde_json::from_str::<UsageSnapshot>(&t).ok())
        .map(|mut s| {
            if !s.windows.is_empty() {
                s.status = "stale".into();
            }
            s
        })
        .unwrap_or_default()
}

fn persist(s: &UsageSnapshot) {
    if let Ok(t) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(store_path(), t);
    }
}

pub fn present() -> bool {
    store_url().map(|p| p.is_file()).unwrap_or(false)
}

// ---------------- SQLite, read only ----------------

/// mode=ro first, immutable=1 as the fallback (see the module doc)
fn open_ro(path: &std::path::Path) -> Option<rusqlite::Connection> {
    use rusqlite::OpenFlags;
    if !path.is_file() {
        return None;
    }
    if let Ok(c) = rusqlite::Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        // Actually verify that reads work (with the -shm missing, open can succeed and the first query fail)
        if c.prepare("SELECT 1 FROM ItemTable LIMIT 1").and_then(|mut s| s.query([]).map(|_| ())).is_ok() {
            return Some(c);
        }
    }
    // Only the URI form takes immutable=1; a Windows path becomes file:///C:/... with \ → /
    let mut uri = String::from("file:///");
    uri.push_str(&path.to_string_lossy().replace('\\', "/").trim_start_matches('/').replace('#', "%23").replace('?', "%3F"));
    uri.push_str("?immutable=1");
    rusqlite::Connection::open_with_flags(
        &uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()
}

fn item(conn: &rusqlite::Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM ItemTable WHERE key = ?1", [key], |r| r.get::<_, String>(0))
        .ok()
        .filter(|s| !s.is_empty())
}

struct Creds {
    cookie: String,
    plan: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum CredentialError {
    StoreUnavailable,
    InvalidSession,
    EncryptedSession,
}

impl CredentialError {
    fn note(&self) -> &'static str {
        match self {
            Self::StoreUnavailable => "Sign in to Cursor (the editor) to see usage.",
            Self::InvalidSession => "Cursor's saved session is invalid. Sign in again in the editor.",
            Self::EncryptedSession => "Cursor's saved session could not be decrypted. Open Cursor and sign in again.",
        }
    }
}

/// Both components must remain one HTTP cookie value. Do not infer a token format: older
/// editors can store opaque plaintext tokens, while an encrypted envelope is never a token.
fn valid_cookie_part(value: &str) -> bool {
    !value.is_empty()
        && !contains_storage_namespace(value)
        && value.bytes().all(|b| matches!(b, 0x21..=0x7e) && !matches!(b, b';' | b',' | b'"' | b'\\'))
}

fn contains_storage_namespace(value: &str) -> bool {
    value.as_bytes().windows(SAFE_STORAGE_NAMESPACE.len())
        .any(|part| part.eq_ignore_ascii_case(SAFE_STORAGE_NAMESPACE.as_bytes()))
}

/// Electron and Chromium write padded, standard base64. Validate that framing before
/// reusing our decoder, which also accepts permissive formats needed by other providers.
fn decode_storage_base64(value: &str) -> Option<Vec<u8>> {
    let body = value.trim_end_matches('=');
    let padding = value.len() - body.len();
    if value.is_empty() || value.len() % 4 != 0 || padding > 2 || body.is_empty() {
        return None;
    }
    let sextet = |b| match b {
        b'A'..=b'Z' => Some(b - b'A'),
        b'a'..=b'z' => Some(b - b'a' + 26),
        b'0'..=b'9' => Some(b - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    if !body.bytes().all(|b| sextet(b).is_some()) {
        return None;
    }
    let last = sextet(*body.as_bytes().last()?)?;
    if (padding == 1 && last & 0b11 != 0) || (padding == 2 && last & 0b1111 != 0) {
        return None;
    }
    crate::antigravity::b64_decode(value)
}

/// Chromium's Windows OSCrypt sync format, with empty associated data. A saved token must
/// contain at least one ciphertext byte in addition to the version, nonce and full tag.
fn windows_payload_parts(payload: &[u8]) -> Option<(&[u8], &[u8], &[u8])> {
    let body = payload.strip_prefix(b"v10")?;
    if body.len() <= 12 + 16 {
        return None;
    }
    let (nonce, sealed) = body.split_at(12);
    let (ciphertext, tag) = sealed.split_at(sealed.len() - 16);
    Some((nonce, ciphertext, tag))
}

fn decode_access_token_with(
    saved: &str,
    decrypt: impl FnOnce(&[u8]) -> Option<Vec<u8>>,
) -> Result<String, CredentialError> {
    let reserved_namespace = saved.trim().get(..SAFE_STORAGE_NAMESPACE.len())
        .map(|prefix| prefix.eq_ignore_ascii_case(SAFE_STORAGE_NAMESPACE)).unwrap_or(false);
    let token = if reserved_namespace {
        let encoded = saved.strip_prefix(SAFE_STORAGE_V1).ok_or(CredentialError::EncryptedSession)?;
        let payload = decode_storage_base64(encoded).ok_or(CredentialError::EncryptedSession)?;
        windows_payload_parts(&payload).ok_or(CredentialError::EncryptedSession)?;
        let plaintext = decrypt(&payload).ok_or(CredentialError::EncryptedSession)?;
        String::from_utf8(plaintext).map_err(|_| CredentialError::EncryptedSession)?
    } else {
        saved.to_string()
    };
    if !valid_cookie_part(&token) {
        return Err(CredentialError::InvalidSession);
    }
    Ok(token)
}

/// Bind the encryption key to the database we actually opened. Never search another
/// Cursor install, profile, browser cookie store or account when this key is unavailable.
fn local_state_path(store: &Path) -> Option<PathBuf> {
    store.ancestors().nth(3).map(|root| root.join("Local State"))
}

#[cfg(any(windows, test))]
fn protected_key(local_state: &[u8]) -> Option<Vec<u8>> {
    let state: serde_json::Value = serde_json::from_slice(local_state).ok()?;
    let encoded = state.pointer("/os_crypt/encrypted_key")?.as_str()?;
    let bytes = decode_storage_base64(encoded)?;
    let protected = bytes.strip_prefix(b"DPAPI")?;
    (!protected.is_empty()).then(|| protected.to_vec())
}

#[cfg(windows)]
fn decrypt_access_token(payload: &[u8], local_state: &Path) -> Option<Vec<u8>> {
    let (nonce, ciphertext, tag) = windows_payload_parts(payload)?;
    let protected = protected_key(&std::fs::read(local_state).ok()?)?;
    let mut key = windows_safe_storage::unprotect(&protected)?;
    let result = windows_safe_storage::decrypt(&key, nonce, ciphertext, tag);
    windows_safe_storage::clear(&mut key);
    result
}

#[cfg(not(windows))]
fn decrypt_access_token(_payload: &[u8], _local_state: &Path) -> Option<Vec<u8>> {
    None
}

// Electron safeStorage delegates to OSCrypt. The format and key framing are documented by:
// https://github.com/electron/electron/blob/v38.0.0/shell/browser/api/electron_api_safe_storage.cc
// https://github.com/chromium/chromium/blob/136.0.7103.48/components/os_crypt/sync/os_crypt_win.cc
// Windows CNG supplies AES-GCM; no replacement cryptography or key is persisted here.
#[cfg(windows)]
mod windows_safe_storage {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::*;

    pub(super) fn clear(bytes: &mut [u8]) {
        for byte in bytes {
            // Keep erasure of the DPAPI result and our temporary key out of dead-store removal.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
    }

    pub(super) fn unprotect(bytes: &[u8]) -> Option<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: bytes.len().try_into().ok()?,
            pbData: bytes.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            // No extra entropy and no prompt, matching Chromium's key protection.
            CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output).ok()?;
            let result = if !output.pbData.is_null() && output.cbData > 0 {
                let plaintext = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
                let result = plaintext.to_vec();
                clear(plaintext);
                Some(result)
            } else {
                None
            };
            let _ = LocalFree(HLOCAL(output.pbData.cast()));
            result
        }
    }

    struct Algorithm(BCRYPT_ALG_HANDLE);
    impl Drop for Algorithm {
        fn drop(&mut self) {
            unsafe { let _ = BCryptCloseAlgorithmProvider(self.0, 0); }
        }
    }

    pub(super) struct AesKey {
        key: BCRYPT_KEY_HANDLE,
        _algorithm: Algorithm,
    }

    impl Drop for AesKey {
        fn drop(&mut self) {
            unsafe { let _ = BCryptDestroyKey(self.key); }
        }
    }

    impl AesKey {
        pub(super) fn new(bytes: &[u8]) -> Option<Self> {
            if bytes.len() != 32 {
                return None;
            }
            unsafe {
                let mut handle = BCRYPT_ALG_HANDLE::default();
                BCryptOpenAlgorithmProvider(&mut handle, BCRYPT_AES_ALGORITHM, PCWSTR::null(), BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0)).ok().ok()?;
                let algorithm = Algorithm(handle);
                let mode = std::slice::from_raw_parts(
                    BCRYPT_CHAIN_MODE_GCM.as_ptr().cast::<u8>(),
                    (BCRYPT_CHAIN_MODE_GCM.len() + 1) * 2,
                );
                BCryptSetProperty(BCRYPT_HANDLE(handle.0), BCRYPT_CHAINING_MODE, mode, 0).ok().ok()?;
                let mut key = BCRYPT_KEY_HANDLE::default();
                // Windows owns the key-object allocation and releases it in BCryptDestroyKey.
                BCryptGenerateSymmetricKey(handle, &mut key, None, bytes, 0).ok().ok()?;
                Some(Self { key, _algorithm: algorithm })
            }
        }

        fn auth_info(nonce: &mut [u8], tag: &mut [u8]) -> BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO {
            BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO {
                cbSize: std::mem::size_of::<BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO>() as u32,
                dwInfoVersion: BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO_VERSION,
                pbNonce: nonce.as_mut_ptr(),
                cbNonce: nonce.len() as u32,
                pbTag: tag.as_mut_ptr(),
                cbTag: tag.len() as u32,
                ..Default::default()
            }
        }

        #[cfg(test)]
        pub(super) fn encrypt_fixture(&self, nonce: &[u8; 12], plaintext: &[u8]) -> Vec<u8> {
            let mut nonce = *nonce;
            let mut tag = [0u8; 16];
            let mut auth = Self::auth_info(&mut nonce, &mut tag);
            let mut ciphertext = vec![0; plaintext.len()];
            let mut written = 0;
            unsafe {
                BCryptEncrypt(self.key, Some(plaintext), Some((&mut auth as *mut BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO).cast()), None, Some(&mut ciphertext), &mut written, BCRYPT_FLAGS(0)).ok().unwrap();
            }
            assert_eq!(written as usize, plaintext.len());
            let mut payload = b"v10".to_vec();
            payload.extend_from_slice(&nonce);
            payload.extend_from_slice(&ciphertext);
            payload.extend_from_slice(&tag);
            payload
        }
    }

    pub(super) fn decrypt(key: &[u8], nonce: &[u8], ciphertext: &[u8], tag: &[u8]) -> Option<Vec<u8>> {
        if nonce.len() != 12 || tag.len() != 16 || ciphertext.is_empty() || ciphertext.len() > u32::MAX as usize {
            return None;
        }
        let key = AesKey::new(key)?;
        let mut nonce = nonce.to_vec();
        let mut tag = tag.to_vec();
        let mut auth = AesKey::auth_info(&mut nonce, &mut tag);
        let mut plaintext = vec![0; ciphertext.len()];
        let mut written = 0;
        let status = unsafe {
            BCryptDecrypt(key.key, Some(ciphertext), Some((&mut auth as *mut BCRYPT_AUTHENTICATED_CIPHER_MODE_INFO).cast()), None, Some(&mut plaintext), &mut written, BCRYPT_FLAGS(0))
        };
        if status.is_err() || written as usize != plaintext.len() {
            clear(&mut plaintext);
            return None;
        }
        Some(plaintext)
    }
}

/// Re-read every time: the editor rotates the token, and holding on to an old value signs us out
fn read_credentials() -> Result<Creds, CredentialError> {
    read_credentials_at(&store_url().ok_or(CredentialError::StoreUnavailable)?)
}

fn read_credentials_at(path: &Path) -> Result<Creds, CredentialError> {
    let conn = open_ro(path).ok_or(CredentialError::StoreUnavailable)?;
    let saved = item(&conn, "cursorAuth/accessToken").ok_or(CredentialError::StoreUnavailable)?;
    let token = decode_access_token_with(&saved, |payload| {
        decrypt_access_token(payload, &local_state_path(path)?)
    })?;
    let auth_id = item(&conn, "cursorAuth/stripeMembershipAuthId").ok_or(CredentialError::StoreUnavailable)?;
    if !valid_cookie_part(&auth_id) {
        return Err(CredentialError::InvalidSession);
    }
    let plan = item(&conn, "cursorAuth/stripeMembershipType");
    Ok(Creds { cookie: format!("WorkosCursorSessionToken={auth_id}::{token}"), plan })
}

/// For doctor: contains no secret values
pub fn probe() -> String {
    let Some(p) = store_url() else { return "Cursor: cannot locate %APPDATA%".into() };
    if !p.is_file() {
        return format!("Cursor: {} not found (not installed, or not signed in)", p.display());
    }
    match read_credentials() {
        Ok(c) => format!(
            "Cursor: session borrowed (cookie {} chars, plan={})",
            c.cookie.len(),
            c.plan.unwrap_or_else(|| "?".into())
        ),
        Err(error) => format!("Cursor: {}", error.note()),
    }
}

// ---------------- Parsing ----------------

fn pct(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64()).map(|p| (p / 100.0).clamp(0.0, 1.0))
}

fn parse_iso(v: Option<&serde_json::Value>) -> Option<u64> {
    v.and_then(|x| x.as_str())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis().max(0) as u64)
}

/// usage-summary → (windows, note). When there are no windows the note says why (Unlimited / free plan without an allowance)
pub fn parse_summary(v: &serde_json::Value) -> (Vec<LimitWindow>, String) {
    let resets_at = parse_iso(v.get("billingCycleEnd"));
    let usage = v.get("individualUsage").cloned().unwrap_or(serde_json::Value::Null);
    let plan = usage.get("plan").cloned().unwrap_or(serde_json::Value::Null);
    let mut out = Vec::new();
    // Headline = the dashboard number; 0 is a reading too
    if let Some(total) = pct(plan.get("totalPercentUsed")) {
        out.push(LimitWindow { id: "included".into(), label: "Included usage".into(), used: total, resets_at, ..Default::default() });
    }
    if let Some(api) = pct(plan.get("apiPercentUsed")) {
        if api > 0.0 {
            out.push(LimitWindow { id: "api".into(), label: "API usage".into(), used: api, resets_at, ..Default::default() });
        }
    }
    if let Some(od) = usage.get("onDemand") {
        let enabled = od.get("enabled").and_then(|x| x.as_bool()).unwrap_or(false);
        let limit = od.get("limit").and_then(|x| x.as_f64()).unwrap_or(0.0);
        let used = od.get("used").and_then(|x| x.as_f64());
        if enabled && limit > 0.0 {
            if let Some(u) = used {
                out.push(LimitWindow {
                    id: "on_demand".into(),
                    label: "On demand".into(),
                    used: (u / limit).clamp(0.0, 1.0),
                    resets_at, ..Default::default()
                });
            }
        }
    }
    if !out.is_empty() {
        return (out, String::new());
    }
    let membership = v.get("membershipType").and_then(|x| x.as_str()).unwrap_or("this");
    let note = if v.get("isUnlimited").and_then(|x| x.as_bool()) == Some(true) {
        format!("Unlimited on the {membership} plan — nothing to meter")
    } else {
        format!("The {membership} plan has nothing for Cursor to meter yet")
    };
    (out, note)
}

#[derive(Debug)]
enum FetchErr {
    NeedsAuth,
    Other(String),
}

fn fetch_once(cookie: &str) -> Result<serde_json::Value, FetchErr> {
    fetch_at(ENDPOINT, cookie)
}

fn fetch_at(endpoint: &str, cookie: &str) -> Result<serde_json::Value, FetchErr> {
    // Final guard at the network boundary, including future envelope versions.
    if contains_storage_namespace(cookie) {
        return Err(FetchErr::NeedsAuth);
    }
    let agent = crate::http::builder().timeout(Duration::from_secs(15)).build();
    match agent.get(endpoint).set("Cookie", cookie).set("Accept", "application/json").call() {
        Ok(r) => r.into_json::<serde_json::Value>().map_err(|e| FetchErr::Other(format!("parse: {e}"))),
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => Err(FetchErr::NeedsAuth),
        Err(ureq::Error::Status(code, _)) => Err(FetchErr::Other(format!("HTTP {code}"))),
        Err(e) => Err(FetchErr::Other(format!("{e}"))),
    }
}

fn cap(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn read_once(prev: &UsageSnapshot) -> UsageSnapshot {
    let mut snap = prev.clone();
    let creds = match read_credentials() {
        Ok(creds) => creds,
        Err(error) => {
            snap.status = "needsAuth".into();
            snap.note = error.note().into();
            return snap;
        }
    };
    match fetch_once(&creds.cookie) {
        Ok(v) => {
            let (windows, note) = parse_summary(&v);
            snap.fetched_at = now_ms();
            if windows.is_empty() {
                snap.status = "none".into();
                snap.windows.clear();
                snap.note = note;
            } else {
                snap.status = "ok".into();
                snap.windows = windows;
                snap.note = match (&creds.plan, v.get("membershipType").and_then(|x| x.as_str())) {
                    (_, Some(m)) => format!("{} · via Cursor", cap(m)),
                    (Some(p), None) => format!("{} · via Cursor", cap(p)),
                    _ => String::new(),
                };
            }
        }
        Err(FetchErr::NeedsAuth) => {
            snap.status = "needsAuth".into();
            snap.note = "Cursor session was rejected — sign in again in the editor".into();
        }
        Err(FetchErr::Other(msg)) => {
            // Stale beats invented: keep the old reading, marked stale
            snap.status = if snap.windows.is_empty() { "error" } else { "stale" }.into();
            snap.note = msg;
        }
    }
    snap
}

fn broadcast(app: &AppHandle, snap: UsageSnapshot) {
    let st = app.state::<AppState>();
    *st.cursor.lock().unwrap() = snap.clone();
    persist(&snap);
    let _ = app.emit("cursor", &snap);
}

fn sleep_interruptible(secs: u64) {
    for _ in 0..secs {
        if REFRESH.swap(false, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

pub fn start(app: AppHandle) {
    std::thread::spawn(move || {
        {
            let st = app.state::<AppState>();
            let snap = st.cursor.lock().unwrap().clone();
            let _ = app.emit("cursor", &snap);
        }
        if !present() {
            broadcast(&app, UsageSnapshot { status: "absent".into(), ..Default::default() });
            loop {
                sleep_interruptible(600); // Cursor is not installed: look again every 10 minutes
                if present() {
                    break;
                }
            }
        }
        loop {
            let prev = {
                let st = app.state::<AppState>();
                let s = st.cursor.lock().unwrap().clone();
                s
            };
            let snap = read_once(&prev);
            if snap.status == "error" || snap.status == "stale" {
                crate::applog(&format!("cursor: {}", snap.note));
            }
            broadcast(&app, snap);
            sleep_interruptible(POLL_SECS);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = ((chunk[0] as u32) << 16)
                | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
                | chunk.get(2).copied().unwrap_or(0) as u32;
            out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
            out.push(if chunk.len() > 1 { ALPHABET[((n >> 6) & 63) as usize] as char } else { '=' });
            out.push(if chunk.len() > 2 { ALPHABET[(n & 63) as usize] as char } else { '=' });
        }
        out
    }

    fn payload() -> Vec<u8> {
        let mut bytes = b"v10".to_vec();
        bytes.extend(0..12);
        bytes.extend_from_slice(b"synthetic ciphertext");
        bytes.extend(32..48);
        bytes
    }

    fn wrapped(payload: &[u8]) -> String {
        format!("{SAFE_STORAGE_V1}{}", encode(payload))
    }

    #[test]
    fn storage_base64_accepts_standard_encoding_and_rejects_corrupt_framing() {
        for (encoded, plaintext) in [("TQ==", "M"), ("TWE=", "Ma"), ("TWFu", "Man")] {
            assert_eq!(decode_storage_base64(encoded), Some(plaintext.as_bytes().to_vec()));
        }
        for malformed in ["", "A", "TQ", "TQ=", "TQ===", "=TQ=", "T=Q=", "TR==", "TWF=", "TQ==\n", " TQ==", "____", "----", "!!!!"] {
            assert_eq!(decode_storage_base64(malformed), None, "accepted {malformed:?}");
        }
    }

    #[test]
    fn legacy_token_needs_no_decryptor_or_local_state() {
        assert_eq!(decode_access_token_with("legacy.opaque.token", |_| panic!("plaintext must not decrypt")), Ok("legacy.opaque.token".into()));
    }

    #[test]
    fn encrypted_token_uses_only_authenticated_plaintext() {
        let bytes = payload();
        let decoded = decode_access_token_with(&wrapped(&bytes), |actual| {
            assert_eq!(actual, bytes);
            Some(b"synthetic.access.token".to_vec())
        });
        assert_eq!(decoded, Ok("synthetic.access.token".into()));
    }

    #[test]
    fn unsupported_or_malformed_envelopes_never_reach_decryptor() {
        for saved in ["cursor-safe-storage:v2:AAAA", "cursor-safe-storage:v1:", "cursor-safe-storage:v1:!!!!", "cursor-safe-storage:v1:djEw", "cursor-safe-storage:legacy", "CURSOR-SAFE-STORAGE:v1:AAAA", " cursor-safe-storage:v2:AAAA", "\tcursor-safe-storage:v1:AAAA"] {
            assert_eq!(decode_access_token_with(saved, |_| panic!("invalid envelope must not decrypt")), Err(CredentialError::EncryptedSession));
        }
        for prefix in [b"v11", b"v20", b"v99"] {
            let mut bytes = payload();
            bytes[..3].copy_from_slice(prefix);
            assert_eq!(decode_access_token_with(&wrapped(&bytes), |_| panic!("unsupported version must not decrypt")), Err(CredentialError::EncryptedSession));
        }
    }

    #[test]
    fn decrypt_failure_utf8_and_nested_envelopes_never_become_tokens() {
        let saved = wrapped(&payload());
        assert_eq!(decode_access_token_with(&saved, |_| None), Err(CredentialError::EncryptedSession));
        assert_eq!(decode_access_token_with(&saved, |_| Some(vec![0xff])), Err(CredentialError::EncryptedSession));
        for plaintext in ["", "cursor-safe-storage:v1:AAAA", "CURSOR-SAFE-STORAGE:v1:AAAA", "token;other=value", "token\r\nCookie: forged", "token with spaces"] {
            assert_eq!(decode_access_token_with(&saved, |_| Some(plaintext.as_bytes().to_vec())), Err(CredentialError::InvalidSession));
            assert!(!valid_cookie_part(plaintext));
        }
    }

    #[test]
    fn windows_payload_requires_nonce_ciphertext_and_full_tag() {
        let bytes = payload();
        let (nonce, ciphertext, tag) = windows_payload_parts(&bytes).unwrap();
        assert_eq!(nonce, (0..12).collect::<Vec<_>>());
        assert_eq!(ciphertext, b"synthetic ciphertext");
        assert_eq!(tag, (32..48).collect::<Vec<_>>());
        for length in 0..=31 {
            assert!(windows_payload_parts(&bytes[..length]).is_none());
        }
    }

    #[test]
    fn local_state_requires_its_explicit_dpapi_key() {
        let state = serde_json::json!({ "os_crypt": { "encrypted_key": encode(b"DPAPIsynthetic protected key") } });
        assert_eq!(protected_key(&serde_json::to_vec(&state).unwrap()), Some(b"synthetic protected key".to_vec()));
        for state in [serde_json::json!({}), serde_json::json!({"os_crypt":{"encrypted_key":true}}), serde_json::json!({"os_crypt":{"encrypted_key":"!!!!"}}), serde_json::json!({"os_crypt":{"encrypted_key":encode(b"APPBunsupported")}}), serde_json::json!({"os_crypt":{"encrypted_key":encode(b"DPAPI")}})] {
            assert_eq!(protected_key(&serde_json::to_vec(&state).unwrap()), None);
        }
        assert_eq!(protected_key(b"not JSON"), None);
    }

    #[test]
    fn key_path_is_bound_to_the_opened_store() {
        let root = PathBuf::from("fixture").join("Cursor");
        let store = root.join("User").join("globalStorage").join("state.vscdb");
        assert_eq!(local_state_path(&store), Some(root.join("Local State")));
    }

    #[test]
    fn http_boundary_rejects_every_envelope_version_before_connecting() {
        for wrapper in ["cursor-safe-storage:v1:AAAA", "cursor-safe-storage:v2:AAAA", "cursor-safe-storage:unknown:AAAA", "CURSOR-SAFE-STORAGE:v1:AAAA", " cursor-safe-storage:v2:AAAA"] {
            let cookie = format!("WorkosCursorSessionToken=fixture::{wrapper}");
            assert!(matches!(fetch_at("invalid endpoint must never be used", &cookie), Err(FetchErr::NeedsAuth)));
        }
    }

    #[test]
    fn http_fixture_receives_decrypted_token_in_existing_cookie_format() {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let endpoint = format!("http://{}/usage-summary", server.server_addr());
        let worker = std::thread::spawn(move || {
            let request = server.recv_timeout(Duration::from_secs(5)).unwrap().expect("fixture request");
            let cookie = request.headers().iter().find(|h| h.field.equiv("Cookie")).unwrap().value.as_str().to_owned();
            assert_eq!(request.url(), "/usage-summary");
            assert_eq!(request.headers().iter().find(|h| h.field.equiv("Accept")).unwrap().value.as_str(), "application/json");
            request.respond(tiny_http::Response::from_string(r#"{"fixture":true}"#)).unwrap();
            cookie
        });
        let token = decode_access_token_with(&wrapped(&payload()), |_| Some(b"synthetic.access.token".to_vec())).unwrap();
        let cookie = format!("WorkosCursorSessionToken=fixture-auth-id::{token}");
        assert_eq!(fetch_at(&endpoint, &cookie).unwrap(), serde_json::json!({ "fixture": true }));
        assert_eq!(worker.join().unwrap(), "WorkosCursorSessionToken=fixture-auth-id::synthetic.access.token");
    }

    struct FixtureDir(PathBuf);
    impl FixtureDir {
        fn new() -> Self {
            // A fresh direct child cannot follow an existing temp/cache symlink. Never use
            // the OS temp directory, APPDATA, an installed editor or a real account profile.
            let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap();
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let path = manifest.join(format!(".cursor-store-fixture-{}-{nanos}", std::process::id()));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn store(&self, saved: &str) -> PathBuf {
            let path = self.0.join("Cursor").join("User").join("globalStorage").join("state.vscdb");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value TEXT);").unwrap();
            for (key, value) in [("cursorAuth/accessToken", saved), ("cursorAuth/stripeMembershipAuthId", "auth0|synthetic-user"), ("cursorAuth/stripeMembershipType", "pro")] {
                conn.execute("INSERT INTO ItemTable (key, value) VALUES (?1, ?2)", [key, value]).unwrap();
            }
            path
        }
    }
    impl Drop for FixtureDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn legacy_fixture_reads_exact_store_without_writing_or_requiring_a_key() {
        let fixture = FixtureDir::new();
        let store = fixture.store("synthetic.legacy.token");
        let before = std::fs::read(&store).unwrap();
        let creds = read_credentials_at(&store).unwrap();
        assert_eq!(creds.cookie, "WorkosCursorSessionToken=auth0|synthetic-user::synthetic.legacy.token");
        assert_eq!(creds.plan.as_deref(), Some("pro"));
        assert_eq!(std::fs::read(&store).unwrap(), before);
        assert!(!local_state_path(&store).unwrap().exists());
    }

    #[cfg(windows)]
    fn protect_fixture_key(key: &[u8]) -> Vec<u8> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{LocalFree, HLOCAL};
        use windows::Win32::Security::Cryptography::*;
        let input = CRYPT_INTEGER_BLOB { cbData: key.len() as u32, pbData: key.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut output).unwrap();
            let mut bytes = b"DPAPI".to_vec();
            bytes.extend_from_slice(std::slice::from_raw_parts(output.pbData, output.cbData as usize));
            let _ = LocalFree(HLOCAL(output.pbData.cast()));
            bytes
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_cng_matches_boringssl_aes256_gcm_vector_and_rejects_tampering() {
        // Public AES-256-GCM known-answer vector, independent of our fixture encryptor:
        // https://boringssl.googlesource.com/boringssl/+/517073cd4b/crypto/cipher/cipher_test.txt
        let ciphertext = [0xce, 0xa7, 0x40, 0x3d, 0x4d, 0x60, 0x6b, 0x6e, 0x07, 0x4e, 0xc5, 0xd3, 0xba, 0xf3, 0x9d, 0x18];
        let mut tag = [0xd0, 0xd1, 0xc8, 0xa7, 0x99, 0x99, 0x6b, 0xf0, 0x26, 0x5b, 0x98, 0xb5, 0xd4, 0x8a, 0xb9, 0x19];
        assert_eq!(windows_safe_storage::decrypt(&[0; 32], &[0; 12], &ciphertext, &tag), Some(vec![0; 16]));
        tag[0] ^= 1;
        assert_eq!(windows_safe_storage::decrypt(&[0; 32], &[0; 12], &ciphertext, &tag), None);
        assert_eq!(windows_safe_storage::decrypt(&[0; 31], &[0; 12], &ciphertext, &tag), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_native_store_roundtrip_is_read_only_and_fails_closed() {
        let fixture = FixtureDir::new();
        let raw_key = [0x41; 32];
        let key = windows_safe_storage::AesKey::new(&raw_key).unwrap();
        let payload = key.encrypt_fixture(&[0x72; 12], b"synthetic.encrypted.token");
        let store = fixture.store(&wrapped(&payload));
        let local_state = local_state_path(&store).unwrap();
        let state = serde_json::json!({ "os_crypt": { "encrypted_key": encode(&protect_fixture_key(&raw_key)) } });
        std::fs::write(&local_state, serde_json::to_vec(&state).unwrap()).unwrap();
        let db_before = std::fs::read(&store).unwrap();
        let key_before = std::fs::read(&local_state).unwrap();
        let creds = read_credentials_at(&store).unwrap();
        assert_eq!(creds.cookie, "WorkosCursorSessionToken=auth0|synthetic-user::synthetic.encrypted.token");
        assert_eq!(creds.plan.as_deref(), Some("pro"));
        assert_eq!(std::fs::read(&store).unwrap(), db_before);
        assert_eq!(std::fs::read(&local_state).unwrap(), key_before);

        let mut corrupt = payload.clone();
        *corrupt.last_mut().unwrap() ^= 1;
        assert_eq!(decode_access_token_with(&wrapped(&corrupt), |p| decrypt_access_token(p, &local_state)), Err(CredentialError::EncryptedSession));
        let wrong_state = serde_json::json!({ "os_crypt": { "encrypted_key": encode(&protect_fixture_key(&[0x42; 32])) } });
        std::fs::write(&local_state, serde_json::to_vec(&wrong_state).unwrap()).unwrap();
        assert!(matches!(read_credentials_at(&store), Err(CredentialError::EncryptedSession)));
        // A valid key somewhere else in the fixture must never rescue this store.
        std::fs::write(fixture.0.join("Local State"), key_before).unwrap();
        std::fs::remove_file(&local_state).unwrap();
        assert!(matches!(read_credentials_at(&store), Err(CredentialError::EncryptedSession)));
    }
}
