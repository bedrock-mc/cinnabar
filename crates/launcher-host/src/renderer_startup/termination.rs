//! Serializes handled termination with sentinel writes, never with native driver calls.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, Weak},
};

#[derive(Default)]
pub(super) struct Active {
    terminating: bool,
    leases: Vec<(PathBuf, Weak<fs::File>)>,
}

static ACTIVE: Mutex<Active> = Mutex::new(Active {
    terminating: false,
    leases: Vec::new(),
});

impl Active {
    /// Records ownership without extending the OS lease beyond the startup guard.
    pub(super) fn register(&mut self, root: &Path, lease: &Arc<fs::File>) {
        self.leases.retain(|(_, lease)| lease.strong_count() != 0);
        self.leases.push((root.to_owned(), Arc::downgrade(lease)));
    }

    /// Revokes future writes and cancels only sentinels still leased by this process.
    fn cancel(&mut self) {
        self.terminating = true;
        for (root, lease) in self.leases.drain(..) {
            if let Some(_lease) = lease.upgrade()
                && let Err(error) = fs::remove_file(root.join(super::SENTINEL_FILE))
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(%error, "could not cancel renderer startup on termination");
            }
        }
    }
}

/// Holds cancellation off during file mutation and rejects work after handled termination.
pub(super) fn active() -> anyhow::Result<MutexGuard<'static, Active>> {
    let active = ACTIVE.lock().unwrap_or_else(|error| error.into_inner());
    anyhow::ensure!(!active.terminating, "client is terminating");
    Ok(active)
}

/// Cancels startup on SIGINT/SIGTERM; panic and native-fault paths retain their sentinel.
pub(crate) fn cancel_on_termination() {
    ACTIVE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .cancel();
}

#[cfg(all(test, unix))]
mod tests;
