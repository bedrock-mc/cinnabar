//! Retained helper and manual-package copies, independent of the replaceable update stage.

use std::{
    ffi::OsString,
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
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

/// Copies the installed `helper` outside the install and starts it with `args`. The helper
/// waits for this process to exit, so the caller must quit promptly; the stdin pipe and the
/// copy's reader lease are released only by that exit.
pub fn start(update_dir: &Path, helper: &Path, args: &[OsString]) -> io::Result<()> {
    prune(update_dir, SystemTime::now());
    let (copy, copy_reader) = self::copy(update_dir, helper)?;
    let directory = copy
        .parent()
        .ok_or_else(|| io::Error::other("missing helper directory"))?;
    let mut command = Command::new(&copy);
    command
        .args(args)
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(File::create(directory.join("apply.log"))?);
    #[cfg(target_os = "linux")]
    for name in crate::startup::APPIMAGE_RUNTIME_ENV {
        command.env_remove(name);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // DETACHED_PROCESS | CREATE_NO_WINDOW
        command.creation_flags(0x00000008 | 0x08000000);
    }
    let mut child = command.spawn()?;
    if let Some(pipe) = child.stdin.take() {
        std::mem::forget(pipe);
    }
    // The helper's shared reader overlaps this one, including a delayed process start.
    std::mem::forget(copy_reader);
    Ok(())
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
        let metadata = fs::metadata(path.join(LOCK));
        let claimed = metadata.as_ref().is_ok_and(|metadata| metadata.len() != 0);
        let handing_off = metadata
            .or_else(|_| entry.metadata())
            .and_then(|metadata| metadata.modified())
            .is_ok_and(|modified| {
                now.duration_since(modified)
                    .map_or(true, |age| age < HANDOFF_WINDOW)
            });
        if !claimed && handing_off {
            continue;
        }
        retire_idle_copy(&path, now);
    }
}

/// Rechecks the handoff time under the exclusive lease, then retires the idle copy.
fn retire_idle_copy(path: &Path, now: SystemTime) {
    let directory_modified = fs::metadata(path).and_then(|metadata| metadata.modified());
    let (lease, created) = match File::options()
        .write(true)
        .create_new(true)
        .open(path.join(LOCK))
    {
        Ok(lease) => (lease, true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => match open_lock(path) {
            Ok(lease) => (lease, false),
            Err(_) => return,
        },
        Err(_) => return,
    };
    if lease.try_lock().is_err() {
        return;
    }
    let Ok(metadata) = lease.metadata() else {
        return;
    };
    let modified = if created {
        directory_modified
    } else {
        metadata.modified()
    };
    if metadata.len() == 0
        && modified.is_ok_and(|modified| {
            now.duration_since(modified)
                .map_or(true, |age| age < HANDOFF_WINDOW)
        })
    {
        return;
    }
    let Some(update_dir) = path.parent() else {
        return;
    };
    if let Ok(retired) = tempfile::Builder::new()
        .prefix(&format!("{PREFIX}discard-"))
        .tempdir_in(update_dir)
    {
        #[cfg(windows)]
        {
            // A nonempty lease fences future readers before its handle closes. Windows
            // cannot rename a directory containing an open file, including this lease.
            if lease.set_len(1).is_err() {
                return;
            }
            drop(lease);
        }
        let _ = fs::rename(path, retired.path().join("copy"));
        // Unix can rename with the exclusive lease held, then close it before removal.
        #[cfg(not(windows))]
        drop(lease);
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

/// Takes a reader lease and rejects a retired copy even while its Windows cleanup is blocked.
fn hold_directory(directory: &Path) -> io::Result<File> {
    let reader = open_lock(directory)?;
    reader.lock_shared()?;
    if !directory.is_dir() || reader.metadata()?.len() != 0 {
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
