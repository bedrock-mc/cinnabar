//! Serializes shared stages, downloads, installation swaps and recovery.

use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    time::Duration,
};

/// Longest a Restart helper waits for the worker or a previous helper's transaction.
pub const WAIT_TIMEOUT: Duration = Duration::from_secs(30);
/// The helper's edge code may sleep between these nonblocking acquisition attempts.
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Owns update checks and consent for one client until its process exits.
#[derive(Debug)]
pub struct Updater {
    _file: File,
}

impl Updater {
    /// Attempts the process-lifetime role without waiting for another client to exit.
    pub fn try_acquire(root: &Path) -> io::Result<Self> {
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("updater.lock"))?;
        file.try_lock()?;
        Ok(Self { _file: file })
    }
}

/// An exclusive lease on one update directory; its lifetime bounds every shared mutation.
#[derive(Debug)]
pub struct Lease {
    root: PathBuf,
    _file: File,
}

impl Lease {
    /// Waits for shared-directory access on a background worker.
    pub fn acquire(root: &Path) -> io::Result<Self> {
        let file = open(root)?;
        file.lock()?;
        Ok(Self {
            root: root.to_owned(),
            _file: file,
        })
    }

    /// Takes shared-directory access only if no other client or helper holds it.
    pub fn try_acquire(root: &Path) -> io::Result<Self> {
        let file = open(root)?;
        file.try_lock()?;
        Ok(Self {
            root: root.to_owned(),
            _file: file,
        })
    }

    /// Waits at most `timeout`, using the caller's monotonic elapsed time and wait callback.
    pub fn wait(
        root: &Path,
        timeout: Duration,
        mut elapsed: impl FnMut() -> Duration,
        mut pause: impl FnMut(),
    ) -> io::Result<Self> {
        let file = open(root)?;
        loop {
            match file.try_lock().map_err(io::Error::from) {
                Ok(()) => {
                    return Ok(Self {
                        root: root.to_owned(),
                        _file: file,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
            if elapsed() >= timeout {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "another updater is busy",
                ));
            }
            pause();
        }
    }

    /// The only directory whose shared data this lease permits the caller to change.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Opens the common lease without truncating it or replacing its identity.
fn open(root: &Path) -> io::Result<File> {
    File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(root.join(crate::LOCK_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn only_one_client_updates_until_its_lifetime_role_is_released() {
        let root = tempfile::tempdir().unwrap();
        let first = Updater::try_acquire(root.path()).unwrap();
        assert_eq!(
            Updater::try_acquire(root.path()).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        // The role is separate from the worker/helper transaction lease.
        let _stage = Lease::acquire(root.path()).unwrap();
        drop(first);
        let second = Updater::try_acquire(root.path()).unwrap();
        assert!(Updater::try_acquire(root.path()).is_err());
        drop(second);
    }

    #[test]
    fn contention_wait_is_bounded_by_the_callers_clock() {
        let root = tempfile::tempdir().unwrap();
        let _other = Lease::acquire(root.path()).unwrap();
        let elapsed = Cell::new(Duration::ZERO);
        let error = Lease::wait(
            root.path(),
            WAIT_TIMEOUT,
            || elapsed.get(),
            || {
                elapsed.set(elapsed.get() + POLL_INTERVAL);
            },
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(elapsed.get(), WAIT_TIMEOUT);
    }

    #[test]
    fn a_waiting_restart_takes_the_lease_when_the_other_client_finishes() {
        let root = tempfile::tempdir().unwrap();
        let other = RefCell::new(Some(Lease::acquire(root.path()).unwrap()));
        let elapsed = Cell::new(Duration::ZERO);
        let lease = Lease::wait(
            root.path(),
            WAIT_TIMEOUT,
            || elapsed.get(),
            || {
                elapsed.set(elapsed.get() + POLL_INTERVAL);
                other.borrow_mut().take();
            },
        )
        .unwrap();
        assert!(Lease::try_acquire(root.path()).is_err());
        drop(lease);
        assert!(Lease::try_acquire(root.path()).is_ok());
    }
}
