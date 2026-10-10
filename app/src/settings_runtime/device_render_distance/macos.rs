//! Dedicated VRAM from the registry entry selected by Metal.

use std::ffi::{c_char, c_void};
type CfRef = *const c_void;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IORegistryEntryIDMatching(id: u64) -> CfRef;
    fn IOServiceGetMatchingService(port: u32, matching: CfRef) -> u32;
    fn IORegistryEntrySearchCFProperty(
        entry: u32,
        plane: *const c_char,
        key: CfRef,
        allocator: CfRef,
        options: u32,
    ) -> CfRef;
    fn IOObjectRelease(object: u32) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringCreateWithCString(allocator: CfRef, bytes: *const c_char, encoding: u32) -> CfRef;
    fn CFGetTypeID(value: CfRef) -> usize;
    fn CFDataGetTypeID() -> usize;
    fn CFDataGetLength(data: CfRef) -> isize;
    fn CFDataGetBytePtr(data: CfRef) -> *const u8;
    fn CFNumberGetTypeID() -> usize;
    fn CFNumberGetValue(number: CfRef, kind: isize, value: *mut c_void) -> u8;
    fn CFRelease(value: CfRef);
}

/// Reads the selected GPU's VRAM property and releases every acquired registry/CF object.
pub(super) fn dedicated_bytes(registry_id: u64) -> Option<u64> {
    // SAFETY: all names are terminated; returned objects are checked, typed and released below.
    unsafe {
        let matching = IORegistryEntryIDMatching(registry_id);
        if matching.is_null() {
            return None;
        }
        let entry = IOServiceGetMatchingService(0, matching);
        if entry == 0 {
            return None;
        }
        let key = CFStringCreateWithCString(std::ptr::null(), c"VRAM,totalMB".as_ptr(), 0x08000100);
        if key.is_null() {
            IOObjectRelease(entry);
            return None;
        }
        let value =
            IORegistryEntrySearchCFProperty(entry, c"IOService".as_ptr(), key, std::ptr::null(), 3);
        CFRelease(key);
        IOObjectRelease(entry);
        if value.is_null() {
            return None;
        }
        let mib = if CFGetTypeID(value) == CFDataGetTypeID() && CFDataGetLength(value) == 4 {
            let mut bytes = [0; 4];
            std::ptr::copy_nonoverlapping(CFDataGetBytePtr(value), bytes.as_mut_ptr(), bytes.len());
            Some(u32::from_ne_bytes(bytes) as u64)
        } else if CFGetTypeID(value) == CFNumberGetTypeID() {
            let mut number = 0i64;
            (CFNumberGetValue(value, 4, (&raw mut number).cast()) != 0)
                .then_some(number)
                .and_then(|number| u64::try_from(number).ok())
        } else {
            None
        };
        CFRelease(value);
        mib.and_then(|mib| mib.checked_mul(1024 * 1024))
    }
}
