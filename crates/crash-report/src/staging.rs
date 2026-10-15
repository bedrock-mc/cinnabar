//! A running client's staging directory, `crashes/.live-<session id>/`: what the client keeps
//! on disk while it runs so a report written after any crash can read it, namely the packet
//! header ring, the log flight ring and rejected packet samples. The client and monitor share
//! a lock until neither can need the attachments.
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
        let lock = lock(&dir.join(LOCK_FILE), true, true)?;
        Ok(Self { dir, _lock: lock })
    }

    /// Retains an existing client's staging directory while the monitor reads its reports.
    pub fn retain(dir: &Path) -> io::Result<Self> {
        let lock = lock(&dir.join(LOCK_FILE), false, true)?;
        Ok(Self {
            dir: dir.to_owned(),
            _lock: lock,
        })
    }

    /// The directory whose attachments this lease protects.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

/// Deletes staging directories only after both client and monitor release them. A lock
/// that is held, or a directory that has no lock yet, stays.
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
        if let Ok(_lock) = lock(&dir.join(LOCK_FILE), false, false) {
            let _ = fs::remove_dir_all(&dir);
        }
    }
}

/// Opens the lock file, creating it only for a new client. Shared leases protect readers;
/// pruning needs an exclusive lease and keeps it until deletion has finished.
fn lock(path: &Path, create: bool, shared: bool) -> io::Result<fs::File> {
    let file = fs::File::options()
        .read(true)
        .write(true)
        .create_new(create)
        .open(path)?;
    if shared {
        file.try_lock_shared().map_err(io::Error::from)?;
    } else {
        file.try_lock().map_err(io::Error::from)?;
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
    fn diagnostics_audit_monitor_keeps_staging_until_attachments_are_read() {
        let crash_dir = tempfile::tempdir().unwrap();
        let client = Staging::create(crash_dir.path(), "session").unwrap();
        fs::write(client.dir().join(PACKET_RING_FILE), b"ring").unwrap();
        let monitor = Staging::retain(client.dir()).unwrap();
        drop(client);
        prune(crash_dir.path());
        assert_eq!(
            fs::read(monitor.dir().join(PACKET_RING_FILE)).unwrap(),
            b"ring"
        );
        drop(monitor);
        prune(crash_dir.path());
        assert!(!path(crash_dir.path(), "session").exists());
    }

    #[test]
    fn a_second_staging_for_a_live_session_fails() {
        let crash_dir = tempfile::tempdir().unwrap();
        let _live = Staging::create(crash_dir.path(), "same").unwrap();
        assert!(Staging::create(crash_dir.path(), "same").is_err());
    }
}
