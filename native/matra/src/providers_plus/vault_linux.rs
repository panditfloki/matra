//! Linux Secret Service adapter. Secrets never enter argv, environment, plaintext files or logs.
//! Background search deliberately omits SECRET_SEARCH_UNLOCK. A locked collection returns no key.
use std::ffi::{c_char, c_int, c_uint, c_void, CString};
type Ptr = *mut c_void;
#[repr(C)]
struct List { data: Ptr, next: *mut List, prev: *mut List }
#[link(name = "secret-1")]
extern "C" {
    fn secret_service_get_sync(flags: c_int, cancellable: Ptr, error: *mut Ptr) -> Ptr;
    fn secret_service_search_sync(service: Ptr, schema: Ptr, attributes: Ptr, flags: c_int, cancellable: Ptr, error: *mut Ptr) -> *mut List;
    fn secret_item_get_locked(item: Ptr) -> c_int;
    fn secret_item_get_secret(item: Ptr) -> Ptr;
    fn secret_value_get(value: Ptr, length: *mut usize) -> *const c_char;
    fn secret_value_unref(value: Ptr);
    fn secret_password_storev_sync(schema: Ptr, attributes: Ptr, collection: *const c_char, label: *const c_char,
        password: *const c_char, cancellable: Ptr, error: *mut Ptr) -> c_int;
    fn secret_password_clearv_sync(schema: Ptr, attributes: Ptr, cancellable: Ptr, error: *mut Ptr) -> c_int;
}
#[link(name = "glib-2.0")]
extern "C" {
    fn g_hash_table_new(hash: Option<unsafe extern "C" fn(*const c_void) -> c_uint>, equal: Option<unsafe extern "C" fn(*const c_void, *const c_void) -> c_int>) -> Ptr;
    fn g_str_hash(value: *const c_void) -> c_uint;
    fn g_str_equal(left: *const c_void, right: *const c_void) -> c_int;
    fn g_hash_table_insert(table: Ptr, key: Ptr, value: Ptr) -> c_int;
    fn g_hash_table_unref(table: Ptr);
    fn g_list_free(list: *mut List);
    fn g_error_free(error: Ptr);
}
#[link(name = "gobject-2.0")]
extern "C" { fn g_object_unref(object: Ptr); }

struct Attributes { table: Ptr, _strings: Vec<CString> }
impl Attributes {
    fn new(id: &str) -> Result<Self, String> {
        let values = ["application", crate::release_channel::BUNDLE_ID, "account", &super::target(id)]
            .into_iter().map(CString::new).collect::<Result<Vec<_>, _>>().map_err(|_| super::UNAVAILABLE)?;
        let table = unsafe { g_hash_table_new(Some(g_str_hash), Some(g_str_equal)) };
        if table.is_null() { return Err(super::UNAVAILABLE.into()); }
        for pair in values.chunks_exact(2) { unsafe { g_hash_table_insert(table, pair[0].as_ptr() as Ptr, pair[1].as_ptr() as Ptr); } }
        Ok(Self { table, _strings: values })
    }
}
impl Drop for Attributes { fn drop(&mut self) { unsafe { g_hash_table_unref(self.table); } } }
struct Error(Ptr);
impl Drop for Error { fn drop(&mut self) { if !self.0.is_null() { unsafe { g_error_free(self.0); } } } }
struct Object(Ptr);
impl Drop for Object { fn drop(&mut self) { if !self.0.is_null() { unsafe { g_object_unref(self.0); } } } }
struct Items(*mut List);
impl Drop for Items {
    fn drop(&mut self) { unsafe {
        let mut item = self.0;
        while !item.is_null() { g_object_unref((*item).data); item = (*item).next; }
        g_list_free(self.0);
    } }
}
struct Value(Ptr);
impl Drop for Value { fn drop(&mut self) { unsafe { secret_value_unref(self.0); } } }

pub fn read(id: &str) -> Option<String> {
    if crate::platform::preview() { return None; }
    let attributes = Attributes::new(id).ok()?;
    let mut error = Error(std::ptr::null_mut());
    // SECRET_SERVICE_OPEN_SESSION = 2. Opening an encrypted session does not unlock collections.
    let service = Object(unsafe { secret_service_get_sync(2, std::ptr::null_mut(), &mut error.0) });
    if service.0.is_null() || !error.0.is_null() { return None; }
    // ALL=2 and LOAD_SECRETS=8. UNLOCK=4 is intentionally absent, including on locked-key errors.
    let items = Items(unsafe { secret_service_search_sync(service.0, std::ptr::null_mut(), attributes.table, 10, std::ptr::null_mut(), &mut error.0) });
    if !error.0.is_null() || items.0.is_null() { return None; }
    unsafe {
        // Reject ambiguous duplicate slots instead of borrowing an arbitrary credential.
        if !(*items.0).next.is_null() || secret_item_get_locked((*items.0).data) != 0 { return None; }
        let value = secret_item_get_secret((*items.0).data);
        if value.is_null() { return None; }
        let value = Value(value);
        let mut length = 0;
        let bytes = secret_value_get(value.0, &mut length);
        if bytes.is_null() || length == 0 || length > super::MAX_BYTES { return None; }
        let secret = String::from_utf8(std::slice::from_raw_parts(bytes.cast(), length).to_vec()).ok()?;
        super::validate(&secret).ok()?;
        Some(secret)
    }
}

pub fn write(id: &str, secret: &str) -> Result<(), String> {
    crate::platform::require_live("Saving provider credentials")?;
    super::validate(secret)?;
    let attributes = Attributes::new(id)?;
    let mut bytes = CString::new(secret).map_err(|_| super::UNAVAILABLE)?.into_bytes_with_nul();
    let mut error = Error(std::ptr::null_mut());
    let accepted = unsafe { secret_password_storev_sync(std::ptr::null_mut(), attributes.table, std::ptr::null(),
        c"Matra Development 0.2.0 provider key".as_ptr(), bytes.as_ptr().cast(), std::ptr::null_mut(), &mut error.0) };
    bytes.fill(0);
    if accepted == 0 || !error.0.is_null() { Err(super::UNAVAILABLE.into()) } else { Ok(()) }
}

pub fn remove(id: &str) -> Result<(), String> {
    crate::platform::require_live("Removing provider credentials")?;
    let attributes = Attributes::new(id)?;
    let mut error = Error(std::ptr::null_mut());
    unsafe { secret_password_clearv_sync(std::ptr::null_mut(), attributes.table, std::ptr::null_mut(), &mut error.0); }
    // No matching item is already removed. A service/permission failure is never reported as success.
    if error.0.is_null() { Ok(()) } else { Err("The Secret Service could not remove the key. Unlock the keyring and explicitly retry.".into()) }
}
