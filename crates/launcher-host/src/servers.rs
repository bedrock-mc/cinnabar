//! Durable storage for the player's saved server list.
//!
//! Writes are atomic (temp sibling + rename) so an interrupted write can
//! never replace a readable file with torn bytes. Loads validate the saved
//! schema and quarantine unreadable files beside the original instead of
//! silently presenting an empty list.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process,
};

use anyhow::{Context, Result, bail};

use {
    launcher::menu::view::SavedServer,
    launcher::menu::{MAX_SERVER_ADDRESS_BYTES, MAX_SERVER_NAME_BYTES},
};

/// Maximum number of saved-server entries retained from one local file.
pub const MAX_SAVED_SERVERS: usize = 256;
/// Maximum saved-server JSON bytes read or written in one operation.
pub const MAX_SAVED_SERVER_FILE_BYTES: usize = 64 * 1024;

/// Result of reading the saved-server file.
pub struct LoadedServers {
    pub servers: Vec<SavedServer>,
    /// False when this load never obtained authority to replace the existing file.
    pub allow_writes: bool,
    /// Set when the previous file was unreadable and was moved aside.
    pub recovery_message: Option<String>,
}

/// Reads and validates the saved-server file.
///
/// A missing file is the normal first-run state. A file that exists but
/// fails schema validation is renamed to a `.invalid` sibling (replacing any
/// earlier quarantine) and reported through [`LoadedServers::recovery_message`]
/// so the failure is visible instead of silently losing the player's list.
/// Transient read errors return an empty list without touching the file.
pub fn load_servers(path: &Path) -> LoadedServers {
    let bytes = match read_bounded(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LoadedServers {
                servers: Vec::new(),
                allow_writes: true,
                recovery_message: None,
            };
        }
        // A readable file that merely cannot be read right now must be
        // visible instead of silently presenting a fresh install; leaving
        // it untouched lets a later load recover it.
        Err(error) => {
            return LoadedServers {
                servers: Vec::new(),
                allow_writes: false,
                recovery_message: Some(format!("Saved servers could not be read: {error}")),
            };
        }
    };
    if bytes.len() > MAX_SAVED_SERVER_FILE_BYTES {
        return quarantine_invalid(path);
    }
    match serde_json::from_slice::<Vec<SavedServer>>(&bytes) {
        Ok(servers) if servers.len() <= MAX_SAVED_SERVERS && servers.iter().all(schema_valid) => {
            LoadedServers {
                servers,
                allow_writes: true,
                recovery_message: None,
            }
        }
        Ok(_) | Err(_) => quarantine_invalid(path),
    }
}

/// Reads at most one byte beyond the saved-server file ceiling so a growing
/// or hostile local file can be rejected without allocating its full size.
fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(MAX_SAVED_SERVER_FILE_BYTES.saturating_add(1));
    file.take((MAX_SAVED_SERVER_FILE_BYTES as u64).saturating_add(1))
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Quarantines one semantically invalid or over-limit saved-server file.
fn quarantine_invalid(path: &Path) -> LoadedServers {
    match quarantine(path) {
        Ok(quarantine_path) => LoadedServers {
            servers: Vec::new(),
            allow_writes: true,
            recovery_message: Some(format!(
                "Saved servers were unreadable; moved to {}",
                quarantine_path.display()
            )),
        },
        Err(_) => LoadedServers {
            servers: Vec::new(),
            allow_writes: false,
            recovery_message: Some(
                "Saved servers were unreadable and could not be moved aside".to_owned(),
            ),
        },
    }
}

fn schema_valid(server: &SavedServer) -> bool {
    !server.address.is_empty()
        && server.name.len() <= MAX_SERVER_NAME_BYTES
        && server.address.len() <= MAX_SERVER_ADDRESS_BYTES
}

fn quarantine(path: &Path) -> Result<PathBuf> {
    let mut name = path
        .file_name()
        .map(std::ffi::OsString::from)
        .ok_or_else(|| anyhow::anyhow!("{} has no file name", path.display()))?;
    name.push(".invalid");
    let target = path.with_file_name(name);
    fs::rename(path, &target).with_context(|| format!("quarantine {}", path.display()))?;
    Ok(target)
}

/// Validates and atomically writes `servers`, as [`ServerWriter`] does off the frame.
#[cfg(test)]
pub fn save_servers(path: &Path, servers: &[SavedServer]) -> Result<()> {
    validate(servers)?;
    write_servers(path, servers)
}

/// The schema limits [`load_servers`] enforces, checked before any write.
fn validate(servers: &[SavedServer]) -> Result<()> {
    if servers.len() > MAX_SAVED_SERVERS {
        bail!(
            "too many saved servers: {} exceeds {MAX_SAVED_SERVERS}",
            servers.len()
        );
    }
    for server in servers {
        if !schema_valid(server) {
            bail!(
                "refusing to save a server outside the schema limits ({} bytes name, {} bytes address)",
                server.name.len(),
                server.address.len()
            );
        }
    }
    Ok(())
}

/// Atomically replaces the saved-server file.
///
/// The serialized list is staged in a pid-suffixed temp sibling, flushed to
/// disk, then renamed over the target, so a crash mid-write leaves the
/// previous complete file intact. A stale temp from an earlier crashed run
/// is removed first, so a recycled pid can never wedge future saves. On
/// POSIX a power loss after the rename may resurrect the older complete
/// file (no directory fsync); torn state is impossible either way.
fn write_servers(path: &Path, servers: &[SavedServer]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context(format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(servers).context("encode saved servers")?;
    if bytes.len() > MAX_SAVED_SERVER_FILE_BYTES {
        bail!(
            "saved-server file is {} bytes, exceeding {MAX_SAVED_SERVER_FILE_BYTES}",
            bytes.len()
        );
    }
    let temp = path.with_file_name(format!(
        "{}.tmp-{}",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        process::id()
    ));
    // One writer thread owns this file, so clearing a leftover temp
    // from an earlier crashed run is safe and keeps create_new working when
    // the OS recycles that pid.
    let _ = fs::remove_file(&temp);
    let write_result = (|| {
        #[cfg(unix)]
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)
                .with_context(|| format!("create {}", temp.display()))?
        };
        #[cfg(not(unix))]
        let mut file =
            fs::File::create(&temp).with_context(|| format!("create {}", temp.display()))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .with_context(|| format!("write {}", temp.display()))
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error).with_context(|| format!("publish {}", path.display()));
    }
    Ok(())
}

enum Job {
    Save(Vec<SavedServer>),
    #[cfg(test)]
    Flush(crossbeam_channel::Sender<()>),
}

/// Persists saved-server snapshots on a worker so the frame never waits on
/// `sync_all`; writes stay ordered and only the newest pending one runs.
#[derive(Debug)]
pub struct ServerWriter {
    jobs: Option<crossbeam_channel::Sender<Job>>,
    errors: crossbeam_channel::Receiver<String>,
    worker: Option<std::thread::JoinHandle<()>>,
    allow_writes: bool,
}

impl ServerWriter {
    /// Keeps failed-load snapshots read-only so they cannot overwrite recoverable data.
    pub fn new(path: PathBuf, allow_writes: bool) -> Self {
        let (jobs, queue) = crossbeam_channel::unbounded::<Job>();
        let (report, errors) = crossbeam_channel::unbounded();
        let worker = std::thread::Builder::new()
            .name("saved-servers".to_owned())
            .spawn(move || {
                let run = |job| match job {
                    Job::Save(servers) => {
                        if let Err(error) = write_servers(&path, &servers) {
                            let _ = report.send(format!("{error:#}"));
                        }
                    }
                    #[cfg(test)]
                    Job::Flush(ack) => {
                        let _ = ack.send(());
                    }
                };
                while let Ok(mut job) = queue.recv() {
                    // A newer snapshot supersedes a queued one; anything else runs in order.
                    while let Ok(next) = queue.try_recv() {
                        if matches!((&job, &next), (Job::Save(_), Job::Save(_))) {
                            job = next;
                        } else {
                            run(std::mem::replace(&mut job, next));
                        }
                    }
                    run(job);
                }
            })
            .ok();
        Self {
            jobs: Some(jobs),
            errors,
            worker,
            allow_writes,
        }
    }

    /// Queues `servers` for writing; schema violations fail here, I/O later
    /// through [`Self::take_error`].
    pub fn save(&self, servers: &[SavedServer]) -> Result<()> {
        validate(servers)?;
        if !self.allow_writes {
            bail!("saved servers are read-only because their original file could not be recovered");
        }
        let jobs = self
            .jobs
            .as_ref()
            .context("saved-server writer has stopped")?;
        jobs.send(Job::Save(servers.to_vec()))
            .context("saved-server writer has stopped")?;
        Ok(())
    }

    /// A write that failed since the last call.
    pub fn take_error(&self) -> Option<String> {
        self.errors.try_recv().ok()
    }

    /// Blocks until every queued write has finished.
    #[cfg(test)]
    pub fn flush(&self) {
        let (ack, done) = crossbeam_channel::bounded(1);
        if let Some(jobs) = &self.jobs
            && jobs.send(Job::Flush(ack)).is_ok()
        {
            let _ = done.recv();
        }
    }
}

impl Drop for ServerWriter {
    /// Finishes queued writes so a save right before exit still lands.
    fn drop(&mut self) {
        drop(self.jobs.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_server_writer_refuses_saves() {
        for disconnected in [false, true] {
            let (send, receive) = crossbeam_channel::unbounded();
            drop(receive);
            let (_, errors) = crossbeam_channel::unbounded();
            let writer = ServerWriter {
                jobs: disconnected.then_some(send),
                errors,
                worker: None,
                allow_writes: true,
            };
            assert!(writer.save(&[]).is_err());
        }
    }
}

#[cfg(test)]
#[path = "servers/tests.rs"]
mod storage_tests;
