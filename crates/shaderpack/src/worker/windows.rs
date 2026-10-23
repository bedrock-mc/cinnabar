//! Owns a compiler-only Windows job with memory, CPU and lifetime limits.

use super::{MEMORY_LIMIT, TIME_LIMIT};
use std::{
    io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    process::Child,
};
use windows_sys::Win32::System::JobObjects::*;

pub(super) struct Job {
    _handle: OwnedHandle,
}

impl Job {
    /// Limits this child before its parent supplies any pack input.
    pub(super) fn attach(child: &Child) -> io::Result<Self> {
        // SAFETY: no name or inheritable security attributes are supplied.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: this is a new non-null owned job handle, closed exactly once by OwnedHandle.
        let job = Self {
            _handle: unsafe { OwnedHandle::from_raw_handle(handle) },
        };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_PROCESS_TIME
            | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
        limits.BasicLimitInformation.PerProcessUserTimeLimit =
            TIME_LIMIT.as_secs() as i64 * 10_000_000;
        limits.ProcessMemoryLimit = MEMORY_LIMIT as usize;
        // SAFETY: the descriptor matches its class and size; both handles remain owned here.
        unsafe {
            if SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle()) == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(job)
    }
}
