//! iOS cannot start processes, so a core command runs in this process instead: the app links
//! the Go core as a C archive, and each core runs on a thread of its own with pipes standing in
//! for the standard streams a child would have.

use std::{
    ffi::{CString, c_char, c_int},
    io,
    os::{
        fd::{IntoRawFd, OwnedFd},
        unix::{ffi::OsStrExt, process::ExitStatusExt},
    },
    process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU32, Ordering},
    },
    thread,
};

unsafe extern "C" {
    /// The linked core's entry (`core/cmd/bedrock-core/embed_ios.go`): runs one core with the
    /// `argc` arguments in `argv` and returns its exit code. It adopts the descriptors, `-1` for
    /// none; EOF on `cancel` stops the core.
    fn CinnabarCoreRun(
        argc: c_int,
        argv: *const *const c_char,
        stdin: c_int,
        stdout: c_int,
        stderr: c_int,
        cancel: c_int,
    ) -> c_int;
}

/// A core running in this process.
#[derive(Debug)]
pub(super) struct Core {
    id: u32,
    pub(super) stdin: Option<ChildStdin>,
    pub(super) stdout: Option<ChildStdout>,
    pub(super) stderr: Option<ChildStderr>,
    /// Closing it stops the core, as SIGTERM stops a process.
    cancel: Option<OwnedFd>,
    exit: Arc<OnceLock<c_int>>,
}

impl Core {
    /// Runs `command`'s arguments (its program is this app) on a thread of its own, with pipes
    /// for the requested streams; output that is not piped is discarded.
    pub(super) fn start(
        command: &Command,
        stdin: bool,
        stdout: bool,
        stderr: bool,
    ) -> io::Result<Self> {
        static NEXT_ID: AtomicU32 = AtomicU32::new(1);
        let args = command
            .get_args()
            .map(|arg| CString::new(arg.as_bytes()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "core argument holds NUL"))?;
        // Each pair is (read end, write end): the core reads stdin and cancel, the host the rest.
        let (stdin_core, stdin_host) = pipe(stdin)?.unzip();
        let (stdout_host, stdout_core) = pipe(stdout)?.unzip();
        let (stderr_host, stderr_core) = pipe(stderr)?.unzip();
        let (cancel_core, cancel_host) = pipe(true)?.unzip();
        let argc = c_int::try_from(args.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many core arguments"))?;
        let exit = Arc::new(OnceLock::new());
        let finished = Arc::clone(&exit);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        thread::Builder::new()
            .name(format!("core-{id}"))
            .spawn(move || {
                let argv: Vec<*const c_char> = args.iter().map(|arg| arg.as_ptr()).collect();
                let raw = |fd: Option<OwnedFd>| fd.map_or(-1, IntoRawFd::into_raw_fd);
                // SAFETY: `argv` points into `args`, which outlives the call, and each
                // descriptor is owned here and handed to the core, which closes it.
                let code = unsafe {
                    CinnabarCoreRun(
                        argc,
                        argv.as_ptr(),
                        raw(stdin_core),
                        raw(stdout_core),
                        raw(stderr_core),
                        raw(cancel_core),
                    )
                };
                let _ = finished.set(code);
            })?;
        Ok(Self {
            id,
            stdin: stdin_host.map(ChildStdin::from),
            stdout: stdout_host.map(ChildStdout::from),
            stderr: stderr_host.map(ChildStderr::from),
            cancel: cancel_host,
            exit,
        })
    }

    /// Numbers the core among this process's cores, where a child would have its process id.
    pub(super) fn id(&self) -> u32 {
        self.id
    }

    /// The exit status once the core has returned, as if a process had exited with its code.
    pub(super) fn try_wait(&self) -> Option<ExitStatus> {
        self.exit
            .get()
            .map(|code| ExitStatus::from_raw((code & 0xff) << 8))
    }

    /// Stops the core as SIGTERM would; a core that outlives its shutdown grace still returns,
    /// leaving its remaining work to finish in the background.
    pub(super) fn cancel(&mut self) {
        drop(self.cancel.take());
    }
}

/// A new pipe's `(read, write)` ends when `wanted`.
fn pipe(wanted: bool) -> io::Result<Option<(OwnedFd, OwnedFd)>> {
    if !wanted {
        return Ok(None);
    }
    let (read, write) = io::pipe()?;
    Ok(Some((read.into(), write.into())))
}
