//! The player's config files in a browser. Browsers have no file system, so the small JSON files a
//! desktop keeps below its config and data roots live in the page's local storage instead, each
//! under its path, where they survive reloads. A write replaces the whole value at once, so no
//! reader sees half a file.

use std::{io, path::Path};

/// Prefixes each file's storage key, keeping the page's own keys apart.
const KEY_PREFIX: &str = "cinnabar.file:";

fn storage() -> io::Result<web_sys::Storage> {
    web_sys::window()
        .and_then(|window| window.local_storage().ok().flatten())
        .ok_or_else(|| io::Error::new(io::ErrorKind::Unsupported, "local storage is unavailable"))
}

fn key(path: &Path) -> String {
    format!("{KEY_PREFIX}{}", path.display())
}

/// The file at `path`, failing with [`io::ErrorKind::NotFound`] when none was written and with
/// [`io::ErrorKind::InvalidData`] past `limit` bytes.
pub fn read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let text = storage()?
        .get_item(&key(path))
        .map_err(|_| io::Error::other("local storage refused the read"))?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such browser file"))?;
    if text.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "browser file exceeds its byte limit",
        ));
    }
    Ok(text.into_bytes())
}

/// Replaces the file at `path` with `bytes`, which must be text, as every config file is.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "browser files hold text"))?;
    storage()?
        .set_item(&key(path), text)
        .map_err(|_| io::Error::other("local storage is full or refused the write"))
}

/// Removes the file at `path`; removing one that was never written succeeds.
pub fn remove(path: &Path) -> io::Result<()> {
    storage()?
        .remove_item(&key(path))
        .map_err(|_| io::Error::other("local storage refused the removal"))
}
