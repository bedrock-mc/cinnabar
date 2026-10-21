//! Atomic file and directory publication shared by asset and pack storage.

use std::{io, path::Path};

#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
/// Atomically moves a file or directory without replacing an existing destination.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(windows)]
/// Atomically moves a file or directory without replacing an existing destination.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;
    // No MOVEFILE_REPLACE_EXISTING: an existing destination fails the move.
    let (from, to) = (extended_wide(from)?, extended_wide(to)?);
    // SAFETY: both are NUL-terminated UTF-16 buffers that outlive the call.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// NUL-terminated `\\?\` form, so deep install paths are not cut at `MAX_PATH`.
#[cfg(windows)]
fn extended_wide(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    let absolute = std::path::absolute(path)?;
    let text = absolute.as_os_str();
    let prefix: &str = match text.to_str() {
        Some(text) if text.starts_with(r"\\?\") => "",
        Some(text) if text.starts_with(r"\\") => r"\\?\UNC",
        _ => r"\\?\",
    };
    let body = if prefix == r"\\?\UNC" {
        absolute
            .to_str()
            .map_or_else(|| text.to_os_string(), |text| text[1..].into())
    } else {
        text.to_os_string()
    };
    Ok(std::ffi::OsStr::new(prefix)
        .encode_wide()
        .chain(body.encode_wide())
        .chain([0])
        .collect())
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_vendor = "apple",
    windows
)))]
/// Atomically moves a file or directory without replacing an existing destination.
pub fn rename_no_replace(_: &Path, _: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace directory publication is unsupported on this platform",
    ))
}
