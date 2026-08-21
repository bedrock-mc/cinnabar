//! Durable storage for the player's saved server list.
//!
//! Writes are atomic (temp sibling + rename) so an interrupted write can
//! never replace a readable file with torn bytes. Loads validate the saved
//! schema and quarantine unreadable files beside the original instead of
//! silently presenting an empty list.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process,
};

use anyhow::{Context, Result, bail};

use super::{MAX_SERVER_ADDRESS_BYTES, MAX_SERVER_NAME_BYTES, SavedServer};

/// Result of reading the saved-server file.
pub(crate) struct LoadedServers {
    pub(crate) servers: Vec<SavedServer>,
    /// Set when the previous file was unreadable and was moved aside.
    pub(crate) recovery_message: Option<String>,
}

/// Reads and validates the saved-server file.
///
/// A missing file is the normal first-run state. A file that exists but
/// fails schema validation is renamed to a `.invalid` sibling (replacing any
/// earlier quarantine) and reported through [`LoadedServers::recovery_message`]
/// so the failure is visible instead of silently losing the player's list.
/// Transient read errors return an empty list without touching the file.
pub(crate) fn load_servers(path: &Path) -> LoadedServers {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return LoadedServers {
                servers: Vec::new(),
                recovery_message: None,
            };
        }
        // A readable file that merely cannot be read right now must be
        // visible instead of silently presenting a fresh install; leaving
        // it untouched lets a later load recover it.
        Err(error) => {
            return LoadedServers {
                servers: Vec::new(),
                recovery_message: Some(format!("Saved servers could not be read: {error}")),
            };
        }
    };
    match serde_json::from_slice::<Vec<SavedServer>>(&bytes) {
        Ok(servers) if servers.iter().all(schema_valid) => LoadedServers {
            servers,
            recovery_message: None,
        },
        Ok(_) | Err(_) => match quarantine(path) {
            Ok(quarantine_path) => LoadedServers {
                servers: Vec::new(),
                recovery_message: Some(format!(
                    "Saved servers were unreadable; moved to {}",
                    quarantine_path.display()
                )),
            },
            Err(_) => LoadedServers {
                servers: Vec::new(),
                recovery_message: Some(
                    "Saved servers were unreadable and could not be moved aside".to_owned(),
                ),
            },
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

/// Atomically replaces the saved-server file.
///
/// The serialized list is staged in a pid-suffixed temp sibling, flushed to
/// disk, then renamed over the target, so a crash mid-write leaves the
/// previous complete file intact. A stale temp from an earlier crashed run
/// is removed first, so a recycled pid can never wedge future saves. On
/// POSIX a power loss after the rename may resurrect the older complete
/// file (no directory fsync); torn state is impossible either way.
/// Refuses to persist entries that [`load_servers`] would quarantine.
pub(crate) fn save_servers(path: &Path, servers: &[SavedServer]) -> Result<()> {
    for server in servers {
        if !schema_valid(server) {
            bail!(
                "refusing to save a server outside the schema limits ({} bytes name, {} bytes address)",
                server.name.len(),
                server.address.len()
            );
        }
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context(format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    let bytes = serde_json::to_vec_pretty(servers).context("encode saved servers")?;
    let temp = path.with_file_name(format!(
        "{}.tmp-{}",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        process::id()
    ));
    // The menu owns this file single-threaded, so clearing a leftover temp
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
