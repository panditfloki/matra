//! Keys entered by the user live only in Windows Credential Manager.
//! Unit tests never read or modify the signed-in user's credential store.
pub const AVAILABLE: bool = cfg!(all(windows, not(test)));
pub const UNAVAILABLE: &str = "Windows Credential Manager is unavailable. No key was saved.";
const MAX_BYTES: usize = 2560;

fn validate(secret: &str) -> Result<(), String> {
    if secret.is_empty() || secret.len() > MAX_BYTES || secret.chars().any(char::is_control) {
        return Err("Enter a key of at most 2560 bytes without control characters.".into());
    }
    Ok(())
}

pub fn target(id: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("matra-notch/provider/{:x}", Sha256::digest(id.as_bytes()))
}

#[cfg(all(windows, not(test)))]
pub fn read(id: &str) -> Option<String> {
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

#[cfg(not(all(windows, not(test))))]
pub fn read(_id: &str) -> Option<String> { None }
#[cfg(not(all(windows, not(test))))]
pub fn write(_id: &str, secret: &str) -> Result<(), String> { validate(secret)?; Err(UNAVAILABLE.into()) }
#[cfg(not(all(windows, not(test))))]
pub fn remove(_id: &str) -> Result<(), String> { Ok(()) }

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
}
