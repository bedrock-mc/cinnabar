//! The cross-process locks of a store: one exclusive writer, any number of readers, and
//! removal, which excludes both.

use std::{
    fs::{File, OpenOptions, TryLockError},
    io,
    path::{Path, PathBuf},
};

/// Name of the writer's lock file inside each store directory.
pub const LOCK_FILE: &str = "store.lock";
/// Name of the file read-only openers lock shared, so removal can see them.
pub const READERS_LOCK_FILE: &str = "readers.lock";
/// Every lock file of a store directory; resets and removal keep them while held.
pub const LOCK_FILES: [&str; 2] = [LOCK_FILE, READERS_LOCK_FILE];

/// True when `name` is one of [`LOCK_FILES`].
#[must_use]
pub fn is_lock_file(name: &std::ffi::OsStr) -> bool {
    LOCK_FILES.iter().any(|lock| name == *lock)
}

/// A lock on one store file, held while this value lives.
#[derive(Debug)]
pub struct StoreLock {
    _file: File,
    path: PathBuf,
}

impl StoreLock {
    /// Takes the writer's exclusive lock of the store in `dir`, creating its lock files;
    /// `None` when another handle, in this process or another, holds it. Two clients on
    /// one server race for it; the loser opens the store read-only and never writes it.
    /// `dir` must exist.
    pub fn try_acquire(dir: &Path) -> io::Result<Option<Self>> {
        let lock = Self::try_lock(dir, LOCK_FILE, false)?;
        if lock.is_some() {
            // Created by the writer, so readers find it and never add a file.
            open_lock_file(&dir.join(READERS_LOCK_FILE))?;
        }
        Ok(lock)
    }

    /// Takes a shared reader's lock of the store in `dir`; `None` while the store is
    /// being removed. Readers never block the writer's lock.
    pub fn try_acquire_reader(dir: &Path) -> io::Result<Option<Self>> {
        Self::try_lock(dir, READERS_LOCK_FILE, true)
    }

    /// Takes both locks exclusively, as removal does; `None` while a writer or any
    /// reader holds the store open.
    pub fn try_acquire_for_removal(dir: &Path) -> io::Result<Option<[Self; 2]>> {
        let Some(writer) = Self::try_acquire(dir)? else {
            return Ok(None);
        };
        Ok(Self::try_lock(dir, READERS_LOCK_FILE, false)?.map(|readers| [writer, readers]))
    }

    /// The lock file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens or creates `name` in `dir` and tries to lock it, shared or exclusive,
    /// without waiting; `None` when another holder conflicts.
    fn try_lock(dir: &Path, name: &str, shared: bool) -> io::Result<Option<Self>> {
        let path = dir.join(name);
        let file = open_lock_file(&path)?;
        let locked = if shared {
            file.try_lock_shared()
        } else {
            file.try_lock()
        };
        match locked {
            Ok(()) => Ok(Some(Self { _file: file, path })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }
}

/// Opens the lock file at `path`, creating it when missing.
fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_at_a_time_and_release_on_drop() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = StoreLock::try_acquire(dir.path())
            .expect("lock")
            .expect("free lock");
        assert_eq!(first.path(), dir.path().join(LOCK_FILE));
        assert!(
            StoreLock::try_acquire(dir.path()).expect("lock").is_none(),
            "a second handle loses"
        );
        drop(first);
        assert!(StoreLock::try_acquire(dir.path()).expect("lock").is_some());
    }

    #[test]
    fn readers_share_and_block_only_removal() {
        let dir = tempfile::tempdir().expect("temp dir");
        let reader = StoreLock::try_acquire_reader(dir.path())
            .expect("lock")
            .expect("free");
        let other = StoreLock::try_acquire_reader(dir.path())
            .expect("lock")
            .expect("readers share");
        let writer = StoreLock::try_acquire(dir.path())
            .expect("lock")
            .expect("readers never block the writer");
        drop(writer);
        assert!(
            StoreLock::try_acquire_for_removal(dir.path())
                .expect("lock")
                .is_none()
        );
        drop((reader, other));
        let removal = StoreLock::try_acquire_for_removal(dir.path())
            .expect("lock")
            .expect("free");
        assert!(
            StoreLock::try_acquire_reader(dir.path())
                .expect("lock")
                .is_none()
        );
        drop(removal);
    }
}
