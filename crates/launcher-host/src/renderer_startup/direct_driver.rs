//! Android build metadata identifies installed system drivers without entering a native GPU API.

use std::ffi::{CStr, c_char, c_void};

/// Keys recovery to the OS, vendor image and graphics-driver build shipped on the device.
pub(super) fn identity() -> anyhow::Result<String> {
    let system = property(c"ro.build.fingerprint");
    anyhow::ensure!(
        !system.is_empty(),
        "Android build fingerprint is unavailable"
    );
    Ok(serde_json::to_string(&[
        system,
        property(c"ro.vendor.build.fingerprint"),
        property(c"ro.gfx.driver_build_time"),
    ])?)
}

unsafe extern "C" {
    /// Reads an Android property without the legacy short-value buffer limit.
    fn __system_property_read_callback(
        info: *const libc::prop_info,
        callback: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, u32),
        cookie: *mut c_void,
    );
}

/// Copies a property synchronously; optional properties may be absent on older devices.
fn property(name: &CStr) -> String {
    let mut value = String::new();
    // SAFETY: The name is terminated, Android retains the property, and the callback only
    // borrows our output for this call. The reader is available at the minimum Android API.
    unsafe {
        let info = libc::__system_property_find(name.as_ptr());
        if !info.is_null() {
            __system_property_read_callback(
                info,
                copy_property,
                (&mut value as *mut String).cast(),
            );
        }
    }
    value
}

/// Copies Android's terminated property value into the caller's borrowed output.
unsafe extern "C" fn copy_property(
    cookie: *mut c_void,
    _name: *const c_char,
    value: *const c_char,
    _serial: u32,
) {
    // SAFETY: Android invokes this only within property(), with its terminated value and our cookie.
    unsafe {
        *cookie.cast::<String>() = CStr::from_ptr(value).to_string_lossy().into_owned();
    }
}
