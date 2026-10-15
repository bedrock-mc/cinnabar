//! A running client's staging directory, `crashes/.live-<session id>/`: what the client keeps
//! on disk while it runs so a report written after any crash can read it, namely the packet
//! header ring, the log flight ring and rejected packet samples. The client holds a lock file
//! inside for its lifetime, which tells live directories from those a dead client left.
//! Folder listing and pruning ignore staging directories.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The packet header ring file, [`crate::ring::packets`].
pub const PACKET_RING_FILE: &str = "packets.ring";
/// The log flight ring file, [`crate::ring::flight`].
pub const FLIGHT_RING_FILE: &str = "flight.ring";
/// Rejected packet bodies with an `index.txt`, copied into reports under the same name.
pub const DECODE_SAMPLES_DIR: &str = crate::folder::DECODE_SAMPLES_DIR;
const LOCK_FILE: &str = "lock";
const PREFIX: &str = ".live-";

/// The staging directory of session `session_id` under `crash_dir`.
pub fn path(crash_dir: &Path, session_id: &str) -> PathBuf {
    crash_dir.join(format!("{PREFIX}{session_id}"))
}

/// This process's staging directory, live while the value is.
#[derive(Debug)]
pub struct Staging {
    dir: PathBuf,
    _lock: fs::File,
}

impl Staging {
    /// Creates session `session_id`'s staging directory and takes its lock.
    pub fn create(crash_dir: &Path, session_id: &str) -> io::Result<Self> {
        let dir = path(crash_dir, session_id);
        fs::create_dir_all(&dir)?;
        let lock = lock(&dir.join(LOCK_FILE), true)?;
        Ok(Self { dir, _lock: lock })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Deletes the staging directories in `crash_dir` whose client is gone; a directory whose lock
/// is held, or that has none yet, stays.
pub fn prune(crash_dir: &Path) {
    for entry in fs::read_dir(crash_dir).into_iter().flatten().flatten() {
        let staging = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(PREFIX));
        if !staging || !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let dir = entry.path();
        if let Ok(lock) = lock(&dir.join(LOCK_FILE), false) {
            drop(lock);
            let _ = fs::remove_dir_all(&dir);
        }
    }
}

/// Opens the lock file at `path` exclusively, creating it when `create`; fails while another
/// process holds it.
#[cfg(windows)]
fn lock(path: &Path, create: bool) -> io::Result<fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    fs::File::options()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .share_mode(0)
        .open(path)
}

/// Opens the lock file at `path` and takes an exclusive advisory lock, creating it when
/// `create`; fails while another process holds it.
#[cfg(unix)]
fn lock(path: &Path, create: bool) -> io::Result<fs::File> {
    use std::os::fd::AsRawFd;
    let file = fs::File::options()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .open(path)?;
    // SAFETY: flock only reads the open descriptor.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder;

    #[test]
    fn prune_keeps_live_staging_and_removes_what_dead_clients_left() {
        let crash_dir = tempfile::tempdir().unwrap();
        let live = Staging::create(crash_dir.path(), "live").unwrap();
        fs::write(live.dir().join(PACKET_RING_FILE), b"ring").unwrap();
        drop(Staging::create(crash_dir.path(), "dead").unwrap());
        fs::create_dir(path(crash_dir.path(), "starting")).unwrap();
        prune(crash_dir.path());
        assert!(live.dir().join(PACKET_RING_FILE).is_file());
        assert!(!path(crash_dir.path(), "dead").exists());
        assert!(path(crash_dir.path(), "starting").exists());
        assert!(folder::list(crash_dir.path()).is_empty());
        drop(live);
        prune(crash_dir.path());
        assert!(!path(crash_dir.path(), "live").exists());
    }

    #[test]
    fn a_second_staging_for_a_live_session_fails() {
        let crash_dir = tempfile::tempdir().unwrap();
        let _live = Staging::create(crash_dir.path(), "same").unwrap();
        assert!(Staging::create(crash_dir.path(), "same").is_err());
    }
}
