//! Reads the desktop's active power source; unavailable reports assume mains.

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> *const c_void;
        fn IOPSGetProvidingPowerSourceType(info: *const c_void) -> *const c_void;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(value: *const c_void);
        fn CFStringGetCString(
            value: *const c_void,
            buffer: *mut i8,
            size: isize,
            encoding: u32,
        ) -> u8;
    }

    /// Reads IOKit's current supplying source, releasing the owned snapshot once.
    pub(super) fn on_battery() -> bool {
        // SAFETY: the copy call returns an owned CoreFoundation snapshot or null.
        let snapshot = unsafe { IOPSCopyPowerSourcesInfo() };
        if snapshot.is_null() {
            return false;
        }
        // SAFETY: the snapshot is live, and the returned string is borrowed from it.
        let source = unsafe { IOPSGetProvidingPowerSourceType(snapshot) };
        let mut name = [0u8; 32];
        // SAFETY: the non-null source is a CFString and the buffer is writable. 0x0600 is
        // CoreFoundation's ASCII encoding, sufficient for the documented power source names.
        let read = !source.is_null()
            && unsafe {
                CFStringGetCString(
                    source,
                    name.as_mut_ptr().cast(),
                    name.len() as isize,
                    0x0600,
                ) != 0
            };
        // SAFETY: the owned snapshot is released after its borrowed string's last use.
        unsafe { CFRelease(snapshot) };
        read && name.starts_with(b"Battery Power\0")
    }
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

    /// Uses Windows' AC line status only when a system battery is known to be present.
    pub(super) fn on_battery() -> bool {
        // SAFETY: all-zero bytes are valid for this integer-only output record.
        let mut status: SYSTEM_POWER_STATUS = unsafe { std::mem::zeroed() };
        // SAFETY: status is a writable record of the size expected by Windows.
        (unsafe { GetSystemPowerStatus(&raw mut status) != 0 })
            && status.ACLineStatus == 0
            && status.BatteryFlag & 128 == 0
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{fs, path::Path};

    /// Reads the kernel power-supply API without assuming a particular battery device name.
    pub(super) fn on_battery() -> bool {
        from_sysfs(Path::new("/sys/class/power_supply"))
    }

    /// Accepts only a discharging system battery and gives online external power precedence.
    fn from_sysfs(root: &Path) -> bool {
        let Ok(entries) = fs::read_dir(root) else {
            return false;
        };
        let mut discharging = false;
        for entry in entries.flatten() {
            let path = entry.path();
            let read = |field| fs::read_to_string(path.join(field)).unwrap_or_default();
            if read("scope").trim() == "Device" {
                continue;
            }
            if read("type").trim() == "Battery" {
                discharging |= read("status").trim() == "Discharging";
            } else if read("online").trim() == "1" {
                return false;
            }
        }
        discharging
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Creates one kernel-style supply report under an isolated fixture root.
        fn supply(root: &Path, name: &str, fields: &[(&str, &str)]) {
            let path = root.join(name);
            fs::create_dir(&path).unwrap();
            for (key, value) in fields {
                fs::write(path.join(key), value).unwrap();
            }
        }

        #[test]
        fn only_a_discharging_system_battery_selects_the_battery_default() {
            let root = tempfile::tempdir().unwrap();
            assert!(!from_sysfs(root.path()));
            supply(
                root.path(),
                "mouse",
                &[
                    ("type", "Battery"),
                    ("scope", "Device"),
                    ("status", "Discharging"),
                ],
            );
            assert!(!from_sysfs(root.path()));
            supply(
                root.path(),
                "laptop",
                &[
                    ("type", "Battery"),
                    ("status", "Discharging"),
                    ("online", "1"),
                ],
            );
            assert!(from_sysfs(root.path()));
            supply(
                root.path(),
                "adapter",
                &[("type", "Mains"), ("online", "1")],
            );
            assert!(!from_sysfs(root.path()));
            assert!(!from_sysfs(&root.path().join("missing")));
        }
    }
}

/// Reports battery power on supported desktop platforms; unknown sources assume mains.
pub(super) fn on_battery() -> bool {
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    {
        platform::on_battery()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    {
        false
    }
}
