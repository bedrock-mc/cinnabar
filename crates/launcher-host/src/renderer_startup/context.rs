//! Renderer attribution belongs to this process, independently of shared recovery files.

use super::Attempt;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicPtr, Ordering},
    },
};

static ATTEMPTS: Mutex<BTreeMap<PathBuf, Attempt>> = Mutex::new(BTreeMap::new());
static SNAPSHOT: AtomicPtr<BTreeMap<PathBuf, Attempt>> = AtomicPtr::new(std::ptr::null_mut());

/// Publishes this process's pending or completed attempt, clearing returned errors.
pub(super) fn update(root: &Path, attempt: Option<&Attempt>) {
    let mut attempts = ATTEMPTS.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(attempt) = attempt {
        attempts.insert(root.to_owned(), attempt.clone());
    } else {
        attempts.remove(root);
    }
    // Startup publishes only a few snapshots. Retain them until process exit so a faulting
    // thread can read immutable context without locks or memory reclamation in the handler.
    SNAPSHOT.store(Box::into_raw(Box::new(attempts.clone())), Ordering::Release);
    drop(attempts);
    #[cfg(unix)]
    crate::lifecycle::crash::unix::update(attempt);
}

/// Reads an immutable process snapshot without waiting on a faulting thread.
pub(crate) fn current(root: &Path) -> Option<Attempt> {
    let snapshot = SNAPSHOT.load(Ordering::Acquire);
    // SAFETY: Published snapshots are immutable and remain allocated until process exit.
    unsafe { snapshot.as_ref() }?.get(root).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reporting_can_read_context_while_the_update_lock_is_held() {
        let dir = tempfile::tempdir().unwrap();
        let attempt = Attempt {
            backend: launcher::graphics_backend::Backend::Metal,
            adapter: None,
            stage: crate::renderer_startup::Stage::Ready,
            explicit: false,
        };
        update(dir.path(), Some(&attempt));
        let _held = ATTEMPTS.lock().unwrap();
        assert_eq!(current(dir.path()), Some(attempt));
    }
}
