//! Installed CPU topology used when choosing graphics defaults.

/// Zero means the platform could not report that count.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cores {
    pub logical: u32,
    pub physical: u32,
}

/// Reads topology once, without running a workload or counting SMT threads as physical cores.
pub fn cores() -> Cores {
    static CORES: std::sync::OnceLock<Cores> = std::sync::OnceLock::new();
    *CORES.get_or_init(read_cores)
}

/// Queries both installed core counts from the macOS kernel.
#[cfg(target_os = "macos")]
fn read_cores() -> Cores {
    Cores {
        logical: sysctl_count(c"hw.logicalcpu"),
        physical: sysctl_count(c"hw.physicalcpu"),
    }
}

/// Reads a positive integer sysctl, retaining unknown values as zero.
#[cfg(target_os = "macos")]
fn sysctl_count(name: &std::ffi::CStr) -> u32 {
    use std::ffi::{c_char, c_int, c_void};
    unsafe extern "C" {
        fn sysctlbyname(
            name: *const c_char,
            old: *mut c_void,
            old_length: *mut usize,
            new: *mut c_void,
            new_length: usize,
        ) -> c_int;
    }
    let mut count = 0_u32;
    let mut length = size_of::<u32>();
    // SAFETY: the NUL-terminated name and correctly sized integer remain alive during the query.
    let status = unsafe {
        sysctlbyname(
            name.as_ptr(),
            (&raw mut count).cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if status == 0 && length == size_of::<u32>() {
        count
    } else {
        0
    }
}

/// Uses kernel topology records, keeping absent physical IDs unknown.
#[cfg(target_os = "linux")]
fn read_cores() -> Cores {
    std::fs::read_to_string("/proc/cpuinfo")
        .map_or_else(|_| Cores::default(), |text| cpuinfo_cores(&text))
}

/// Counts logical processors and distinct socket/core pairs in Linux CPU information.
#[cfg(any(target_os = "linux", test))]
fn cpuinfo_cores(text: &str) -> Cores {
    let mut logical = 0;
    let mut physical = std::collections::BTreeSet::new();
    let mut topology_records = 0;
    for record in text.split("\n\n") {
        let mut socket = None;
        let mut core = None;
        let mut processor = false;
        for line in record.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            match key.trim() {
                "processor" => processor = value.trim().parse::<u32>().is_ok(),
                "physical id" => socket = value.trim().parse::<u32>().ok(),
                "core id" => core = value.trim().parse::<u32>().ok(),
                _ => {}
            }
        }
        if processor {
            logical += 1;
            if let (Some(socket), Some(core)) = (socket, core) {
                physical.insert((socket, core));
                topology_records += 1;
            }
        }
    }
    Cores {
        logical,
        physical: if topology_records == logical {
            physical.len() as u32
        } else {
            0
        },
    }
}

/// Queries all Windows processor groups, counting one topology record per physical core.
#[cfg(target_os = "windows")]
fn read_cores() -> Cores {
    use windows_sys::Win32::System::{
        SystemInformation::{GetLogicalProcessorInformationEx, RelationProcessorCore},
        Threading::{ALL_PROCESSOR_GROUPS, GetActiveProcessorCount},
    };
    // SAFETY: this query has no pointers and reads the installed active processor count.
    let logical = unsafe { GetActiveProcessorCount(ALL_PROCESSOR_GROUPS) };
    let mut length = 0_u32;
    // SAFETY: a null buffer requests its required size; no output records are read here.
    unsafe {
        GetLogicalProcessorInformationEx(RelationProcessorCore, std::ptr::null_mut(), &mut length);
    }
    if length == 0 || length > 1 << 20 {
        return Cores {
            logical,
            physical: 0,
        };
    }
    let mut storage = vec![0_u64; (length as usize).div_ceil(size_of::<u64>())];
    let capacity = storage.len() * size_of::<u64>();
    // SAFETY: the eight-byte-aligned allocation has at least the requested capacity.
    if unsafe {
        GetLogicalProcessorInformationEx(
            RelationProcessorCore,
            storage.as_mut_ptr().cast(),
            &mut length,
        )
    } == 0
        || length as usize > capacity
    {
        return Cores {
            logical,
            physical: 0,
        };
    }
    let mut offset = 0;
    let mut physical = 0;
    while offset + 2 * size_of::<u32>() <= length as usize {
        // SAFETY: both four-byte header fields are inside the initialized output.
        let header = unsafe { storage.as_ptr().cast::<u8>().add(offset) };
        let relationship = unsafe { header.cast::<i32>().read_unaligned() };
        let size = unsafe { header.add(size_of::<u32>()).cast::<u32>().read_unaligned() } as usize;
        if size < 2 * size_of::<u32>() || offset + size > length as usize {
            return Cores {
                logical,
                physical: 0,
            };
        }
        if relationship == RelationProcessorCore {
            physical += 1;
        }
        offset += size;
    }
    Cores {
        logical,
        physical: if offset == length as usize {
            physical
        } else {
            0
        },
    }
}

/// Unsupported platforms retain unknown physical topology.
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn read_cores() -> Cores {
    Cores {
        logical: std::thread::available_parallelism().map_or(0, |count| count.get() as u32),
        physical: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_smt_threads_and_socket_ids_do_not_duplicate_physical_cores() {
        let text = "processor: 0\nphysical id: 0\ncore id: 0\n\nprocessor: 1\nphysical id: 0\ncore id: 0\n\nprocessor: 2\nphysical id: 1\ncore id: 0\n";
        let cores = cpuinfo_cores(text);
        assert_eq!(cores.logical, 3);
        assert_eq!(cores.physical, 2);
        assert_eq!(cpuinfo_cores("processor: 0\n").physical, 0);
        assert_eq!(cpuinfo_cores("").logical, 0);
    }
}
