//! Borrowed native execution and observation of the host's final process stop.

use tokio::runtime::Handle;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

/// Cloneable execution capabilities; the host retains runtime and process-stop authority.
/// Dropping a clone never cancels work or stops the runtime.
#[derive(Clone, Debug)]
pub struct NativeExecution {
    runtime: Handle,
    tasks: TaskTracker,
    process_stop: CancellationToken,
}

/// The host's sole final-process stop authority; execution clones only observe it.
#[derive(Debug)]
pub struct StopAuthority {
    process_stop: CancellationToken,
}

impl StopAuthority {
    /// Permanently stops the original process scope without claiming graceful completion.
    pub fn stop(&self) {
        self.process_stop.cancel();
    }
}

impl NativeExecution {
    /// Separates the host's stop authority from cloneable existing execution capabilities.
    pub fn for_process(runtime: Handle, tasks: TaskTracker) -> (Self, StopAuthority) {
        let process_stop = CancellationToken::new();
        (
            Self {
                runtime,
                tasks,
                process_stop: process_stop.clone(),
            },
            StopAuthority { process_stop },
        )
    }

    /// Borrows execution without taking ownership of the runtime's lifetime.
    pub fn runtime(&self) -> &Handle {
        &self.runtime
    }

    /// Tracks future destruction without granting process cancellation.
    pub fn tasks(&self) -> &TaskTracker {
        &self.tasks
    }

    /// Observes the sticky final process stop, including one issued before construction.
    pub async fn stopped(&self) {
        self.process_stop.cancelled().await;
    }
}
