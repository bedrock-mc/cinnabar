//! Process identity for attributing a pending renderer attempt to its own crash report.

#[cfg(any(target_os = "linux", target_os = "android", test))]
use anyhow::Context;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// The PID and OS process start time distinguish a process from a later reuse of its PID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(super) struct Owner {
    pid: u32,
    start_time: u64,
}

impl Owner {
    /// Reads only this process's identity; the startup file lease determines exclusive ownership.
    pub(super) fn current() -> Result<Self> {
        Ok(Self {
            pid: std::process::id(),
            start_time: start_time()?,
        })
    }
}

/// Reads the kernel's process start tick after the parenthesized executable name.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn start_time() -> Result<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat")?;
    parse_start_time(&stat)
}

/// Handles executable names containing spaces or closing parentheses in Linux process metadata.
#[cfg(any(target_os = "linux", target_os = "android", test))]
fn parse_start_time(stat: &str) -> Result<u64> {
    stat.rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .context("process start time missing")?
        .parse()
        .context("invalid process start time")
}

/// Reads the kernel's process creation timestamp in microseconds.
#[cfg(target_os = "macos")]
fn start_time() -> Result<u64> {
    let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::uninit();
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: The writable output is exactly the size required for this process-info flavor.
    let written = unsafe {
        libc::proc_pidinfo(
            std::process::id() as i32,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    anyhow::ensure!(
        written == size,
        "read process start time: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: The kernel initialized the entire output structure.
    let info = unsafe { info.assume_init() };
    Ok(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

/// Reads the Windows process creation timestamp without opening another process.
#[cfg(windows)]
fn start_time() -> Result<u64> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };
    let empty = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (empty, empty, empty, empty);
    // SAFETY: The pseudo-handle refers to this process and all four outputs are valid.
    let read = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    anyhow::ensure!(
        read != 0,
        "read process start time: {}",
        std::io::Error::last_os_error()
    );
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// Registers one stable timestamp on targets without a native process-start query.
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    windows
)))]
fn start_time() -> Result<u64> {
    use std::{
        sync::OnceLock,
        time::{SystemTime, UNIX_EPOCH},
    };
    static START: OnceLock<u64> = OnceLock::new();
    Ok(*START.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |time| time.as_secs())
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_start_time_handles_unusual_names() {
        let fields = (3..=21).map(|_| "0").collect::<Vec<_>>().join(" ");
        assert_eq!(
            parse_start_time(&format!("42 (name with ) spaces) {fields} 12345 0")).unwrap(),
            12345
        );
        assert!(parse_start_time("42 (missing fields)").is_err());
        assert_eq!(Owner::current().unwrap(), Owner::current().unwrap());
    }
}
