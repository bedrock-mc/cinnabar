//! Retained helper and manual-package copies, independent of the replaceable update stage.

use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

const PREFIX: &str = "helper-";
/// Retained stages share the existing manual package directory namespace.
const SNAPSHOT_PREFIX: &str = "manual-";
/// Held while a helper process or a manual package handoff uses its copy.
const LOCK: &str = "helper.lock";
/// An idle copy is kept this long while its helper starts or the desktop handler opens it.
pub const HANDOFF_WINDOW: Duration = Duration::from_secs(60);

/// Copies `source` into a fresh directory and returns the helper with its handoff reader lease.
pub fn copy(update_dir: &Path, source: &Path) -> io::Result<(PathBuf, File)> {
    let (directory, reader) = retained_directory(update_dir, PREFIX)?;
    let helper = directory.path().join(crate::helper_filename());
    fs::copy(source, &helper)?;
    let _ = directory.keep();
    Ok((helper, reader))
}

/// Creates a snapshot directory and holds it against pruning until its last reader finishes.
/// Keep the directory after verification and stamp the lease's modification time at handoff.
pub fn snapshot_directory(update_dir: &Path) -> io::Result<(tempfile::TempDir, File)> {
    retained_directory(update_dir, SNAPSHOT_PREFIX)
}

/// Holds an immutable snapshot in both client and helper during their overlapping handoff.
pub fn hold_snapshot(directory: &Path) -> io::Result<File> {
    if !directory
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(SNAPSHOT_PREFIX))
    {
        return Err(io::Error::other(
            "the helper accepts only a retained stage snapshot",
        ));
    }
    hold_directory(directory)
}

/// Holds the running helper's own directory; the updater never runs from the install it replaces.
pub fn hold(executable: &Path) -> io::Result<File> {
    let directory = executable
        .parent()
        .filter(|directory| {
            directory
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(PREFIX))
        })
        .ok_or_else(|| io::Error::other("the updater runs only from its copied directory"))?;
    hold_directory(directory)
}

/// Removes idle helper and manual copies as of `now`, keeping held leases and recent handoffs.
pub fn prune(update_dir: &Path, now: SystemTime) {
    let Ok(entries) = fs::read_dir(update_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let copy = entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(PREFIX) || name.starts_with(SNAPSHOT_PREFIX));
        if !copy {
            continue;
        }
        // A clock that moved backwards leaves the copy for a later pass.
        let path = entry.path();
        let handing_off = fs::metadata(path.join(LOCK))
            .or_else(|_| entry.metadata())
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| {
                now.duration_since(modified)
                    .map_or(true, |age| age < HANDOFF_WINDOW)
            });
        if handing_off {
            continue;
        }
        let lease = match File::options().write(true).open(path.join(LOCK)) {
            Ok(lock) if lock.try_lock().is_ok() => Some(lock),
            Ok(_) => continue,
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(_) => continue,
        };
        // Claim the idle name while still holding its exclusive lease. A later reader cannot
        // enter between the idle check and deletion; drop the file before Windows cleanup.
        if let Ok(retired) = tempfile::Builder::new()
            .prefix(&format!("{PREFIX}discard-"))
            .tempdir_in(update_dir)
        {
            let _ = fs::rename(&path, retired.path().join("copy"));
            drop(lease);
        }
    }
}

/// Creates a private copy and its reader lease before publishing any copied files.
fn retained_directory(update_dir: &Path, prefix: &str) -> io::Result<(tempfile::TempDir, File)> {
    let directory = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(update_dir)?;
    let reader = open_lock(directory.path())?;
    reader.lock_shared()?;
    Ok((directory, reader))
}

/// Takes a reader lease and rejects a copy claimed by pruning before that acquisition finished.
fn hold_directory(directory: &Path) -> io::Result<File> {
    let reader = open_lock(directory)?;
    reader.lock_shared()?;
    if !directory.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "retained copy was already retired",
        ));
    }
    Ok(reader)
}

/// Opens the lease shared by a retained copy's owner and the retention scan.
fn open_lock(directory: &Path) -> io::Result<File> {
    File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(directory.join(LOCK))
}

#[cfg(test)]
mod tests;
