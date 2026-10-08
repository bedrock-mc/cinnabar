//! Private helper boundary shared by supervision and deterministic scheduling tests.

use anyhow::Result;
use mod_host::helper::{Dispatch, Helper, Reply};
use server_experience::runtime::{Capabilities, Principal};
use std::path::Path;

pub(super) trait Worker: Sized {
    /// Starts a helper whose first response is its initialization transaction.
    fn spawn(
        executable: &Path,
        bytes: Vec<u8>,
        owner: Principal,
        capabilities: Capabilities,
        epoch: u64,
    ) -> Result<Self>;
    /// Returns a completed or failed callback without waiting on the render thread; an `Err`
    /// means the helper is gone.
    fn poll(&mut self) -> Option<Result<Reply>>;
    /// Submits one callback to an idle helper.
    fn dispatch(&mut self, request: Dispatch) -> Result<()>;
    /// The helper's stderr lines since the last call.
    fn drain_log(&mut self) -> Vec<String> {
        Vec::new()
    }
}

impl Worker for Helper {
    /// Starts the developer helper with the host-selected capabilities.
    fn spawn(
        executable: &Path,
        bytes: Vec<u8>,
        owner: Principal,
        capabilities: Capabilities,
        epoch: u64,
    ) -> Result<Self> {
        Self::spawn_developer(executable, bytes, owner, capabilities, epoch)
    }

    /// Polls the supervised process without blocking the frame.
    fn poll(&mut self) -> Option<Result<Reply>> {
        self.poll()
    }

    /// Forwards an event only after supervision has admitted its callback.
    fn dispatch(&mut self, request: Dispatch) -> Result<()> {
        self.dispatch(request)
    }

    /// Takes the helper's captured stderr.
    fn drain_log(&mut self) -> Vec<String> {
        self.drain_log()
    }
}
