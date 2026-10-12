//! Bounded extension status messages and atomic file publication.

use super::Status;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

pub(super) const MAX_MESSAGE_BYTES: usize = 512;
pub(super) static WRITE_ID: AtomicU64 = AtomicU64::new(0);

/// Removes controls and bounds a status message on UTF-8 character boundaries.
pub(super) fn bounded_message(message: &str) -> String {
    let mut output = String::new();
    for character in message.chars().filter(|character| !character.is_control()) {
        if output.len() + character.len_utf8() > MAX_MESSAGE_BYTES {
            break;
        }
        output.push(character);
    }
    output
}

/// Replaces a status file atomically after flushing its complete serialized contents.
pub(super) fn write_status(path: &Path, status: &Status) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(status)?;
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("status directory unavailable"))?;
    fs::create_dir_all(parent)?;
    let id = WRITE_ID.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("pending-{}-{id}", std::process::id()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    let _ = fs::remove_file(temporary);
    result
}
