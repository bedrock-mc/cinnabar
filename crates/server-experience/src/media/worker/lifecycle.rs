//! Cleanup ownership while a helper starts and when its asynchronous reaper cannot start.

use super::{DecoderLease, SharedChild};
use anyhow::Result;
use std::{
    io,
    process::{ChildStdin, ChildStdout, Command, Stdio},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) const IPC_THREAD: &str = "experience-media-ipc";
pub(super) const WATCHDOG_THREAD: &str = "experience-media-watchdog";
pub(super) const REAPER_THREAD: &str = "experience-media-reap";
pub(super) type Task = Box<dyn FnOnce() + Send + 'static>;

/// Starts detached helper tasks, permitting deterministic thread-creation failures in tests.
pub(super) trait Threads: Send + Sync {
    fn spawn(&self, name: &'static str, task: Task) -> io::Result<()>;
}

pub(super) struct SystemThreads;

impl Threads for SystemThreads {
    fn spawn(&self, name: &'static str, task: Task) -> io::Result<()> {
        std::thread::Builder::new()
            .name(name.into())
            .spawn(task)
            .map(drop)
    }
}

/// Owns cancellation and synchronous reaping until both required threads have started.
pub(super) struct Starting {
    pub(super) child: SharedChild,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) lease: Arc<DecoderLease>,
    armed: bool,
}

impl Starting {
    /// Arms cleanup immediately after spawning, retaining the decoder slot through failure cleanup.
    pub(super) fn spawn(mut command: Command, cancelled: Arc<AtomicBool>) -> Result<Self> {
        let lease = Arc::new(
            DecoderLease::acquire()
                .ok_or_else(|| anyhow::anyhow!("media decoder already active"))?,
        );
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self {
            child: Arc::new(Mutex::new(Some(child))),
            cancelled,
            lease,
            armed: true,
        })
    }

    /// Transfers the private pipes while cleanup remains armed for subsequent failures.
    pub(super) fn pipes(&self) -> Result<(u32, ChildStdin, ChildStdout)> {
        let mut child = lock_child(&self.child);
        let child = child.as_mut().expect("starting helper is owned");
        let (Some(input), Some(output)) = (child.stdin.take(), child.stdout.take()) else {
            anyhow::bail!("missing media helper pipes");
        };
        Ok((child.id(), input, output))
    }

    /// Hands cleanup to a fully constructed Worker, retaining the IPC thread's lease clone.
    pub(super) fn finish(mut self) -> SharedChild {
        self.armed = false;
        Arc::clone(&self.child)
    }
}

impl Drop for Starting {
    fn drop(&mut self) {
        if self.armed {
            self.cancelled.store(true, Ordering::Release);
            kill(&self.child);
            reap(&self.child);
        }
    }
}

/// Retains cleanup ownership even if another operation poisoned the process mutex.
pub(super) fn lock_child(child: &SharedChild) -> MutexGuard<'_, Option<std::process::Child>> {
    child.lock().unwrap_or_else(|error| error.into_inner())
}

/// Requests termination without transferring the only wait owner.
pub(super) fn kill(child: &SharedChild) {
    if let Some(child) = lock_child(child).as_mut() {
        let _ = child.kill();
    }
}

/// Takes the unique wait owner, then waits without holding the process mutex.
pub(super) fn reap(child: &SharedChild) {
    let owned = lock_child(child).take();
    if let Some(mut child) = owned {
        let _ = child.wait();
    }
}
