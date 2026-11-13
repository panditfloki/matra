//! Keys entered by the user live only in an OS credential store.
//! Unit tests never read or modify the signed-in user's credential store.
pub const AVAILABLE: bool = cfg!(all(any(windows, target_os = "macos", target_os = "linux"), not(test)));
#[cfg(windows)]
pub const UNAVAILABLE: &str = "Windows Credential Manager is unavailable. No key was saved.";
#[cfg(target_os = "macos")]
pub const UNAVAILABLE: &str = "macOS Keychain is unavailable or access was refused. Unlock your login Keychain, then explicitly retry saving the key. No key was saved.";
#[cfg(not(any(windows, target_os = "macos")))]
pub const UNAVAILABLE: &str = "Linux Secret Service is unavailable or locked. Unlock your desktop keyring, then explicitly retry. No key was saved.";
const MAX_BYTES: usize = 2560;

fn validate(secret: &str) -> Result<(), String> {
    if secret.is_empty() || secret.len() > MAX_BYTES || secret.chars().any(char::is_control) {
        return Err("Enter a key of at most 2560 bytes without control characters.".into());
    }
    Ok(())
}

pub fn target(id: &str) -> String {
    use sha2::{Digest, Sha256};
    let namespace = crate::release_channel::DATA_NAMESPACE;
    format!("{namespace}/provider/{:x}", Sha256::digest(id.as_bytes()))
}

#[cfg(all(windows, not(test)))]
pub fn read(id: &str) -> Option<String> {
    if crate::platform::preview() { return None; }
    use windows::core::PCWSTR;
    use windows::Win32::Security::Credentials::{CredReadW,CredFree,CREDENTIALW,CRED_TYPE_GENERIC};
    let name: Vec<u16> = target(id).encode_utf16().chain(Some(0)).collect();
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        if CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, 0, &mut ptr).is_err() || ptr.is_null() { return None; }
        let credential = &*ptr;
        let answer = if credential.CredentialBlobSize as usize > MAX_BYTES || credential.CredentialBlob.is_null() {
            None
        } else {
            String::from_utf8(std::slice::from_raw_parts(credential.CredentialBlob,credential.CredentialBlobSize as usize).to_vec()).ok()
        };
        CredFree(ptr as *const core::ffi::c_void);
        answer.filter(|s| validate(s).is_ok())
    }
}

#[cfg(all(windows, not(test)))]
pub fn write(id: &str, secret: &str) -> Result<(), String> {
    crate::platform::require_live("Saving provider credentials")?;
    use windows::core::PWSTR;
    use windows::Win32::Security::Credentials::{CredWriteW,CREDENTIALW,CRED_TYPE_GENERIC,CRED_PERSIST_LOCAL_MACHINE};
    validate(secret)?;
    let mut name: Vec<u16> = target(id).encode_utf16().chain(Some(0)).collect();
    let mut user: Vec<u16> = "Mātrā".encode_utf16().chain(Some(0)).collect();
    let mut bytes = secret.as_bytes().to_vec();
    let credential = CREDENTIALW {Type:CRED_TYPE_GENERIC,TargetName:PWSTR(name.as_mut_ptr()),
        CredentialBlobSize:bytes.len() as u32,CredentialBlob:bytes.as_mut_ptr(),
        Persist:CRED_PERSIST_LOCAL_MACHINE,UserName:PWSTR(user.as_mut_ptr()),..Default::default()};
    let result = unsafe { CredWriteW(&credential,0) }.map_err(|_| UNAVAILABLE.to_string());
    bytes.fill(0);
    result
}

#[cfg(all(windows, not(test)))]
pub fn remove(id: &str) -> Result<(), String> {
    crate::platform::require_live("Removing provider credentials")?;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{CredDeleteW,CRED_TYPE_GENERIC};
    let name: Vec<u16> = target(id).encode_utf16().chain(Some(0)).collect();
    match unsafe { CredDeleteW(PCWSTR(name.as_ptr()),CRED_TYPE_GENERIC,0) } {
        Ok(()) => Ok(()),
        Err(error) if error.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
        Err(_) => Err("Could not remove the key from Windows Credential Manager.".into()),
    }
}

#[cfg(all(not(test), not(any(windows, target_os = "macos", target_os = "linux"))))]
pub fn read(_id: &str) -> Option<String> { None }
#[cfg(all(not(test), not(any(windows, target_os = "macos", target_os = "linux"))))]
pub fn write(_id: &str, secret: &str) -> Result<(), String> { validate(secret)?; Err(UNAVAILABLE.into()) }
#[cfg(all(not(test), not(any(windows, target_os = "macos", target_os = "linux"))))]
pub fn remove(_id: &str) -> Result<(), String> { Err(UNAVAILABLE.into()) }

#[cfg(test)]
mod test_store {
    //! Never linked to Security.framework or Credential Manager. Per-thread scopes avoid one
    //! parallel unit test seeing another test's synthetic key; dropping a scope restores its state.
    use std::{cell::RefCell, collections::HashMap};
    #[derive(Default)]
    struct State { enabled: bool, keys: HashMap<String, String> }
    thread_local! { static STORE: RefCell<State> = RefCell::new(State::default()); }
    pub struct Scope(State);
    pub fn scope() -> Scope {
        Scope(STORE.with(|s| s.replace(State { enabled: true, ..Default::default() })))
    }
    impl Drop for Scope {
        fn drop(&mut self) { STORE.with(|s| { s.replace(std::mem::take(&mut self.0)); }); }
    }
    pub fn read(id: &str) -> Option<String> { STORE.with(|s| s.borrow().keys.get(id).cloned()) }
    pub fn write(id: &str, secret: &str) -> Result<(), String> {
        super::validate(secret)?;
        STORE.with(|s| {
            let mut s = s.borrow_mut();
            if !s.enabled { return Err(super::UNAVAILABLE.into()); }
            s.keys.insert(id.into(), secret.into()); Ok(())
        })
    }
    pub fn remove(id: &str) -> Result<(), String> {
        STORE.with(|s| {
            let mut s = s.borrow_mut();
            if !s.enabled { return Err(super::UNAVAILABLE.into()); }
            s.keys.remove(id); Ok(())
        })
    }
}
#[cfg(test)]
pub use test_store::{read, write, remove};

#[cfg(all(target_os = "macos", not(test)))]
mod keychain {
    //! Minimal Security.framework bridge. Queries are confined to Mātrā's own generic-password
    //! service/account. Background reads explicitly forbid Keychain authentication UI.
    use std::ffi::c_void;
    type Ref = *const c_void;
    #[repr(C)]
    struct Callbacks<const N: usize> { version: isize, functions: [Ref; N] }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithBytes(allocator: Ref, bytes: *const u8, length: isize, encoding: u32, external: u8) -> Ref;
        fn CFDataCreate(allocator: Ref, bytes: *const u8, length: isize) -> Ref;
        fn CFDictionaryCreate(allocator: Ref, keys: *const Ref, values: *const Ref, count: isize, key_callbacks: Ref, value_callbacks: Ref) -> Ref;
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFDataGetTypeID() -> usize;
        fn CFDataGetLength(value: Ref) -> isize;
        fn CFDataGetBytePtr(value: Ref) -> *const u8;
        fn CFRelease(value: Ref);
        static kCFTypeDictionaryKeyCallBacks: Callbacks<5>;
        static kCFTypeDictionaryValueCallBacks: Callbacks<4>;
        static kCFBooleanTrue: Ref;
    }
    #[link(name = "Security", kind = "framework")]
    extern "C" {
        fn SecItemCopyMatching(query: Ref, result: *mut Ref) -> i32;
        fn SecItemAdd(attributes: Ref, result: *mut Ref) -> i32;
        fn SecItemUpdate(query: Ref, attributes: Ref) -> i32;
        fn SecItemDelete(query: Ref) -> i32;
        static kSecClass: Ref;
        static kSecClassGenericPassword: Ref;
        static kSecAttrService: Ref;
        static kSecAttrAccount: Ref;
        static kSecAttrAccessible: Ref;
        static kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly: Ref;
        static kSecValueData: Ref;
        static kSecReturnData: Ref;
        static kSecMatchLimit: Ref;
        static kSecMatchLimitOne: Ref;
        static kSecUseAuthenticationUI: Ref;
        static kSecUseAuthenticationUIFail: Ref;
    }
    struct Owned(Ref);
    impl Owned {
        fn new(value: Ref) -> Result<Self, String> {
            if value.is_null() { Err(super::UNAVAILABLE.into()) } else { Ok(Self(value)) }
        }
    }
    impl Drop for Owned { fn drop(&mut self) { unsafe { CFRelease(self.0); } } }
    fn text(value: &str) -> Result<Owned, String> {
        // kCFStringEncodingUTF8
        Owned::new(unsafe { CFStringCreateWithBytes(std::ptr::null(), value.as_ptr(), value.len() as isize, 0x08000100, 0) })
    }
    fn dictionary(pairs: &[(Ref, Ref)]) -> Result<Owned, String> {
        let keys: Vec<_> = pairs.iter().map(|p| p.0).collect();
        let values: Vec<_> = pairs.iter().map(|p| p.1).collect();
        Owned::new(unsafe { CFDictionaryCreate(std::ptr::null(), keys.as_ptr(), values.as_ptr(), pairs.len() as isize,
            (&kCFTypeDictionaryKeyCallBacks as *const Callbacks<5>).cast(),
            (&kCFTypeDictionaryValueCallBacks as *const Callbacks<4>).cast()) })
    }
    fn query(id: &str, read: bool) -> Result<Owned, String> {
        let service = text(&super::target(id))?;
        let account = text("Mātrā")?;
        let mut pairs = unsafe { vec![(kSecClass, kSecClassGenericPassword), (kSecAttrService, service.0), (kSecAttrAccount, account.0)] };
        if read { unsafe {
            pairs.extend([(kSecReturnData, kCFBooleanTrue), (kSecMatchLimit, kSecMatchLimitOne),
                (kSecUseAuthenticationUI, kSecUseAuthenticationUIFail)]);
        } }
        dictionary(&pairs)
    }
    pub fn read(id: &str) -> Option<String> {
        if crate::platform::preview() { return None; }
        let query = query(id, true).ok()?;
        let mut result = std::ptr::null();
        let status = unsafe { SecItemCopyMatching(query.0, &mut result) };
        // Copy functions transfer ownership even if a later shape/UTF-8 check fails.
        let result = Owned::new(result).ok()?;
        if status != 0 || unsafe { CFGetTypeID(result.0) != CFDataGetTypeID() } { return None; }
        let length = unsafe { CFDataGetLength(result.0) };
        if length <= 0 || length as usize > super::MAX_BYTES { return None; }
        let ptr = unsafe { CFDataGetBytePtr(result.0) };
        if ptr.is_null() { return None; }
        let value = String::from_utf8(unsafe { std::slice::from_raw_parts(ptr, length as usize) }.to_vec()).ok()?;
        super::validate(&value).ok()?;
        Some(value)
    }
    pub fn write(id: &str, secret: &str) -> Result<(), String> {
        crate::platform::require_live("Saving provider credentials")?;
        super::validate(secret)?;
        let data = Owned::new(unsafe { CFDataCreate(std::ptr::null(), secret.as_ptr(), secret.len() as isize) })?;
        let query = query(id, false)?;
        let changes = dictionary(&unsafe { [(kSecValueData, data.0)] })?;
        let mut status = unsafe { SecItemUpdate(query.0, changes.0) };
        if status == -25300 { // errSecItemNotFound
            let service = text(&super::target(id))?;
            let account = text("Mātrā")?;
            let attributes = dictionary(&unsafe { [(kSecClass, kSecClassGenericPassword), (kSecAttrService, service.0),
                (kSecAttrAccount, account.0), (kSecValueData, data.0),
                (kSecAttrAccessible, kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly)] })?;
            status = unsafe { SecItemAdd(attributes.0, std::ptr::null_mut()) };
        }
        if status == 0 { Ok(()) } else { Err(super::UNAVAILABLE.into()) }
    }
    pub fn remove(id: &str) -> Result<(), String> {
        crate::platform::require_live("Removing provider credentials")?;
        let query = query(id, false)?;
        match unsafe { SecItemDelete(query.0) } {
            0 | -25300 => Ok(()),
            _ => Err("Could not remove the provider key from macOS Keychain. Check Keychain access, then explicitly retry.".into()),
        }
    }
}
#[cfg(all(target_os = "macos", not(test)))]
pub use keychain::{read, write, remove};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn targets_are_separate_without_revealing_server_names() {
        assert_ne!(target("a@https://a.example"),target("a@https://b.example"));
        assert!(!target("a@https://private.example").contains("private"));
    }
    #[test]
    fn rejects_oversized_and_invalid_blobs_without_touching_credentials() {
        assert!(validate(&"x".repeat(2560)).is_ok());
        assert!(validate(&"x".repeat(2561)).is_err());
        assert!(validate("key\nvalue").is_err());
        assert!(!AVAILABLE);
        assert!(write("test", "synthetic").is_err());
    }

    #[test]
    fn synthetic_secret_scope_never_uses_an_os_store_or_leaks_to_another_test() {
        assert!(read("scoped-fixture").is_none());
        {
            let _scope = super::test_store::scope();
            write("scoped-fixture", "synthetic-fixture-key").unwrap();
            assert_eq!(read("scoped-fixture").as_deref(), Some("synthetic-fixture-key"));
            assert!(std::thread::spawn(|| read("scoped-fixture")).join().unwrap().is_none());
            remove("scoped-fixture").unwrap();
            assert!(read("scoped-fixture").is_none());
            write("scoped-fixture", "synthetic-fixture-key").unwrap();
        }
        assert!(read("scoped-fixture").is_none());
        assert!(write("scoped-fixture", "synthetic-fixture-key").is_err());
    }
}

#[cfg(all(target_os = "linux", not(test)))]
#[path = "vault_linux.rs"]
mod linux;
#[cfg(all(target_os = "linux", not(test)))]
pub use linux::{read, write, remove};
