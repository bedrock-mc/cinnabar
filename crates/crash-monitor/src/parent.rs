//! The client process the monitor watches.

/// The client, opened while it is still alive so its exit status stays readable.
pub(crate) struct Parent {
    #[cfg(windows)]
    handle: Option<windows::Handle>,
}

impl Parent {
    pub(crate) fn open(pid: u32) -> Self {
        #[cfg(windows)]
        {
            Self {
                handle: windows::Handle::open(pid),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = pid;
            Self {}
        }
    }

    /// The client's exit code once it has exited. Only Windows lets a process that is not the
    /// parent read it; elsewhere this is `None`.
    pub(crate) fn exit_code(&self) -> Option<u32> {
        #[cfg(windows)]
        {
            self.handle.as_ref()?.exit_code()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

#[cfg(windows)]
mod windows {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0},
        System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, WaitForSingleObject,
        },
    };

    /// How long a client that closed its end of the lifeline may take to finish exiting.
    const EXIT_WAIT_MS: u32 = 10_000;

    pub(super) struct Handle(HANDLE);

    impl Handle {
        pub(super) fn open(pid: u32) -> Option<Self> {
            // SAFETY: Opening a process handle by id has no other preconditions.
            let handle = unsafe {
                OpenProcess(
                    PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                    0,
                    pid,
                )
            };
            (!handle.is_null()).then_some(Self(handle))
        }

        pub(super) fn exit_code(&self) -> Option<u32> {
            // SAFETY: The handle is open with SYNCHRONIZE and query rights for this object's life.
            unsafe {
                if WaitForSingleObject(self.0, EXIT_WAIT_MS) != WAIT_OBJECT_0 {
                    return None;
                }
                let mut code = 0;
                (GetExitCodeProcess(self.0, &mut code) != 0).then_some(code)
            }
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: The handle is owned and closed once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
