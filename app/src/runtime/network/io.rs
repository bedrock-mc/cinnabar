//! Application-owned native reactor, retained beyond every Bevy resource and tracked worker.

use bridge::tasks::{NativeExecution, StopAuthority};
use std::io;
use tokio::runtime::Runtime;
use tokio_util::task::TaskTracker;

/// Declare before App so startup errors/unwind drop resources before their I/O owner.
pub(crate) struct IoOwner {
    runtime: Option<Runtime>,
    execution: NativeExecution,
    stop: StopAuthority,
}

impl IoOwner {
    /// Creates the one explicit session/account reactor; worker count preserves the existing policy.
    pub(crate) fn new() -> io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let (execution, stop) =
            NativeExecution::for_process(runtime.handle().clone(), TaskTracker::new());
        Ok(Self {
            runtime: Some(runtime),
            execution,
            stop,
        })
    }

    /// Borrows execution without transferring or extending the actual Runtime lifetime.
    pub(crate) fn execution(&self) -> NativeExecution {
        self.execution.clone()
    }

    /// Waits outside all frames after root admission/resources stop, under the already armed watchdog.
    pub(crate) fn finish(mut self) {
        self.execution.tasks().close();
        self.stop.stop();
        let runtime = self
            .runtime
            .take()
            .expect("one application runtime shutdown");
        bevy::log::info!(
            shutdown_phase = "io_tasks_wait_begin",
            pending_tasks = self.execution.tasks().len()
        );
        runtime.block_on(self.execution.tasks().wait());
        bevy::log::info!(shutdown_phase = "io_tasks_wait_done");
        bevy::log::info!(shutdown_phase = "io_runtime_shutdown_begin");
        runtime.shutdown_background();
        bevy::log::info!(shutdown_phase = "io_runtime_shutdown_done");
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
