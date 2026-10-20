//! Application-owned native reactor, retained beyond every Bevy resource and tracked worker.

use std::io;
use tokio::runtime::{Handle, Runtime};
use tokio_util::task::TaskTracker;

/// Declare before App so startup errors/unwind drop resources before their I/O owner.
pub(crate) struct IoOwner {
    runtime: Option<Runtime>,
    tasks: TaskTracker,
}

impl IoOwner {
    /// Creates the one explicit session/account reactor; worker count preserves the existing policy.
    pub(crate) fn new() -> io::Result<Self> {
        Ok(Self {
            runtime: Some(
                tokio::runtime::Builder::new_multi_thread()
                    .worker_threads(2)
                    .enable_all()
                    .build()?,
            ),
            tasks: TaskTracker::new(),
        })
    }

    /// Borrows execution without transferring or extending the actual Runtime lifetime.
    pub(crate) fn handle(&self) -> Handle {
        self.runtime
            .as_ref()
            .expect("application runtime retained")
            .handle()
            .clone()
    }

    /// Accounts for worker/future destruction; it does not prove remote-operation success.
    pub(crate) fn tasks(&self) -> TaskTracker {
        self.tasks.clone()
    }

    /// Waits outside all frames after root admission/resources stop, under the already armed watchdog.
    pub(crate) fn finish(mut self) {
        self.tasks.close();
        let runtime = self
            .runtime
            .take()
            .expect("one application runtime shutdown");
        runtime.block_on(self.tasks.wait());
        runtime.shutdown_background();
    }
}

impl Drop for IoOwner {
    /// Unwind/startup failure cannot block or fabricate an orderly cleanup completion.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}
