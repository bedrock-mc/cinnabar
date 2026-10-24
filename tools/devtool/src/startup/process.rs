//! Keeps launch cleanup tied to the child and its isolated process tree.

use std::{
    io,
    process::{Child, Command, Stdio},
};

#[derive(Debug)]
pub(super) struct OwnedClient {
    child: Child,
    #[cfg(windows)]
    job: std::os::windows::io::OwnedHandle,
}

impl OwnedClient {
    /// Starts a client in a private process group, with captured protocol output.
    pub(super) fn spawn(mut command: Command) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_SUSPENDED);
        }
        let child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        #[cfg(windows)]
        let job = match attach_job(&child) {
            Ok(job) => job,
            Err(error) => {
                let mut child = child;
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let owned = Self {
            child,
            #[cfg(windows)]
            job,
        };
        #[cfg(windows)]
        resume(&owned.child)?;
        Ok(owned)
    }
}

impl std::ops::Deref for OwnedClient {
    type Target = Child;
    /// Borrows the exact child owned by this acceptance run.
    fn deref(&self) -> &Child {
        &self.child
    }
}

impl std::ops::DerefMut for OwnedClient {
    /// Keeps process operations attached to the owned child handle.
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

impl Drop for OwnedClient {
    /// Stops descendants even when their wrapper exited while leaving output pipes open.
    fn drop(&mut self) {
        #[cfg(unix)]
        stop_descendants(self.child.id());
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            // SAFETY: this handle owns only the job assigned to our child.
            unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(
                    self.job.as_raw_handle(),
                    1,
                );
            }
        }
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

/// Stops exact PIDs that still belong to the process group this launch created.
#[cfg(unix)]
fn stop_descendants(group: u32) {
    let Ok(output) = Command::new("ps").args(["-axo", "pid=,pgid="]).output() else {
        return;
    };
    for row in String::from_utf8_lossy(&output.stdout).lines() {
        let mut fields = row.split_whitespace();
        let (Some(pid), Some(owner)) = (
            fields.next().and_then(|v| v.parse::<i32>().ok()),
            fields.next().and_then(|v| v.parse::<u32>().ok()),
        ) else {
            continue;
        };
        if pid > 0 && owner == group && pid as u32 != group {
            // SAFETY: positive PID targets one process; membership is rechecked before signaling.
            unsafe {
                if libc::getpgid(pid) == group as i32 {
                    libc::kill(pid, libc::SIGKILL);
                }
            }
        }
    }
}

/// Resumes the suspended child's main thread only after it belongs to the cleanup job.
#[cfg(windows)]
fn resume(child: &Child) -> io::Result<()> {
    use std::os::windows::io::{FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::{Diagnostics::ToolHelp::*, Threading::*},
    };
    // SAFETY: snapshot and thread handles are checked, owned, and closed exactly once.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let _snapshot = OwnedHandle::from_raw_handle(snapshot);
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut found = Thread32First(snapshot, &mut entry);
        while found != 0 {
            if entry.th32OwnerProcessID == child.id() {
                let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if thread.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let _thread = OwnedHandle::from_raw_handle(thread);
                if ResumeThread(thread) == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            found = Thread32Next(snapshot, &mut entry);
        }
        Err(io::Error::other(
            "suspended startup client has no main thread",
        ))
    }
}

/// Assigns the launched client to a job whose descendants cannot outlive acceptance.
#[cfg(windows)]
fn attach_job(child: &Child) -> io::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::System::JobObjects::*;
    // SAFETY: no inherited handle or named job is requested, and all results are checked.
    unsafe {
        let raw = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = OwnedHandle::from_raw_handle(raw);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            raw,
            JobObjectExtendedLimitInformation,
            (&raw const limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) == 0
            || AssignProcessToJobObject(raw, child.as_raw_handle()) == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    #[test]
    fn cleanup_finds_descendants_after_the_wrapper_has_exited() {
        let mut command = Command::new("sh");
        let directory = tempfile::tempdir().unwrap();
        command
            .args([
                "-c",
                "mkfifo \"$1\"; (read line < \"$1\") & echo $!; exit 0",
                "fixture",
            ])
            .arg(directory.path().join("wait"));
        let mut child = OwnedClient::spawn(command).unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let descendant: i32 = line.trim().parse().unwrap();
        child.wait().unwrap();
        // SAFETY: this only queries the exact child PID the wrapper reported.
        assert_eq!(unsafe { libc::getpgid(descendant) }, child.id() as i32);
        drop(child);
        test_time::eventually("owned descendant stopped", || {
            // A killed orphan can remain a zombie until the host's init reaps it.
            let output = Command::new("ps")
                .args(["-o", "stat=", "-p", &descendant.to_string()])
                .output()
                .unwrap();
            let state = String::from_utf8_lossy(&output.stdout);
            state.trim().is_empty() || state.trim().starts_with('Z')
        });
    }
}
