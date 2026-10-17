//! Transfers the monitor's process handle into the helper before the client exits.
use std::{
    io::{self, Read, Write},
    os::windows::io::{AsRawHandle, BorrowedHandle},
    process::Child,
};

use anyhow::{Result, ensure};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, DuplicateHandle, ERROR_INVALID_PARAMETER, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
    },
    System::Threading::{
        GetCurrentProcess, INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    },
};

struct Process(HANDLE);

impl Drop for Process {
    /// Releases the synchronization handle on success and on every error path.
    fn drop(&mut self) {
        // SAFETY: closes the handle owned by this wrapper.
        unsafe { CloseHandle(self.0) };
    }
}

/// Duplicates the original monitor into the helper and sends its handle over the client's pipe.
pub fn transfer_monitor(child: &mut Child, monitor: Option<BorrowedHandle<'_>>) -> io::Result<()> {
    let helper = child.as_raw_handle();
    let pipe = child
        .stdin
        .as_mut()
        .ok_or_else(|| io::Error::other("missing updater exit pipe"))?;
    let mut handle = std::ptr::null_mut();
    if let Some(monitor) = monitor {
        // SAFETY: the process handles and borrowed source handle remain valid during duplication.
        // The duplicate belongs to the helper, survives this process, and is never inherited.
        if unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                monitor.as_raw_handle(),
                helper,
                &mut handle,
                PROCESS_SYNCHRONIZE,
                0,
                0,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
    }
    // A failed write closes stdin when the caller drops the child; the helper then exits and
    // Windows releases any duplicate it received, including a partially delivered handle.
    pipe.write_all(&(handle as u64).to_le_bytes())
}

/// Reads the handle already duplicated into this process; zero means there was no monitor.
fn receive_monitor(pipe: &mut impl Read) -> Result<Option<Process>> {
    let mut bytes = [0; size_of::<u64>()];
    pipe.read_exact(&mut bytes)?;
    let handle = usize::try_from(u64::from_le_bytes(bytes))? as HANDLE;
    ensure!(
        handle != INVALID_HANDLE_VALUE,
        "invalid updater monitor handle"
    );
    Ok(if handle.is_null() {
        None
    } else {
        Some(Process(handle))
    })
}

/// Opens the parent and waits on it and the monitor handle sent by the client.
pub(super) fn wait(pid: u32) -> Result<()> {
    // SAFETY: opens an owned handle with synchronization rights only.
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    let parent = if handle.is_null() {
        // SAFETY: reads the error from the immediately preceding failed OpenProcess.
        ensure!(
            unsafe { GetLastError() } == ERROR_INVALID_PARAMETER,
            "cannot open updater parent"
        );
        None
    } else {
        Some(Process(handle))
    };
    let mut pipe = std::io::stdin().lock();
    let monitor = receive_monitor(&mut pipe)?;
    super::exit_barrier(
        [parent, monitor],
        || {
            pipe.take(1).read_to_end(&mut Vec::new())?;
            Ok(())
        },
        |process| {
            // SAFETY: this synchronization handle remains owned until the wait finishes.
            ensure!(
                unsafe { WaitForSingleObject(process.0, INFINITE) } == WAIT_OBJECT_0,
                "waiting for updater process failed"
            );
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::windows::io::AsHandle,
        process::{Command, Stdio},
    };

    const PROBE: &str = "CINNABAR_MONITOR_HANDLE_PROBE";
    const MONITOR_EXIT: u32 = 42;
    const PROBE_EXIT: i32 = 43;

    /// Checks the monitor after the client drops its source handle and exit pipe.
    #[test]
    fn monitor_handle_probe() {
        let Some(expected) = std::env::var_os(PROBE) else {
            return;
        };
        let mut pipe = std::io::stdin().lock();
        let monitor = receive_monitor(&mut pipe).unwrap();
        pipe.read_to_end(&mut Vec::new()).unwrap();
        if expected == "absent" {
            assert!(monitor.is_none());
        } else {
            let monitor = monitor.unwrap();
            // SAFETY: the client duplicated this handle into the probe before dropping its copy.
            unsafe {
                assert_eq!(WaitForSingleObject(monitor.0, 0), WAIT_OBJECT_0);
            }
        }
        std::process::exit(PROBE_EXIT);
    }

    /// Starts a helper that reads the handle before waiting for the client's exit pipe to close.
    fn probe(expected: &str) -> Child {
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "parent::windows::tests::monitor_handle_probe",
                "--test-threads=1",
            ])
            .env(PROBE, expected)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap()
    }

    #[test]
    fn an_exited_monitor_handle_survives_handoff_and_source_handle_closure() {
        let mut monitor = Command::new("cmd")
            .args(["/Q", "/C", &format!("exit /b {MONITOR_EXIT}")])
            .spawn()
            .unwrap();
        assert_eq!(monitor.wait().unwrap().code(), Some(MONITOR_EXIT as i32));
        let mut helper = probe("present");
        transfer_monitor(&mut helper, Some(monitor.as_handle())).unwrap();
        drop(monitor);
        drop(helper.stdin.take());
        assert_eq!(helper.wait().unwrap().code(), Some(PROBE_EXIT));
    }

    #[test]
    fn handoff_without_a_monitor_does_not_wait_on_another_process() {
        let mut helper = probe("absent");
        transfer_monitor(&mut helper, None).unwrap();
        drop(helper.stdin.take());
        assert_eq!(helper.wait().unwrap().code(), Some(PROBE_EXIT));
    }

    #[test]
    fn a_truncated_monitor_transfer_is_rejected() {
        assert!(receive_monitor(&mut &[][..]).is_err());
        assert!(receive_monitor(&mut &[0; size_of::<u64>() - 1][..]).is_err());
    }
}
