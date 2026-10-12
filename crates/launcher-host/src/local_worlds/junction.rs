use std::{io, path::Path};

#[cfg(any(windows, test))]
const REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
#[cfg(any(windows, test))]
const MAX_REPARSE_BUFFER_BYTES: usize = 16 * 1024;

/// The mount-point reparse buffer aiming a junction at the absolute Windows path `target`. The
/// path travels as UTF-16 data, so no character in it (`&`, `^`, `%`, spaces) needs escaping.
#[cfg(any(windows, test))]
pub(crate) fn junction_reparse_data(target: &str) -> Result<Vec<u8>, &'static str> {
    let target = target.replace('/', "\\");
    let mut print = target.strip_prefix(r"\\?\").unwrap_or(&target).to_owned();
    let bytes = print.as_bytes();
    let substitute = if bytes.len() >= 3
        && bytes[1] == b':'
        && bytes[2] == b'\\'
        && bytes[0].is_ascii_alphabetic()
    {
        format!(r"\??\{print}")
    } else if let Some(share) = print.strip_prefix(r"UNC\") {
        let substitute = format!(r"\??\{print}");
        print = format!(r"\\{share}");
        substitute
    } else if let Some(share) = print.strip_prefix(r"\\").filter(|share| !share.is_empty()) {
        format!(r"\??\UNC\{share}")
    } else {
        return Err("junction target must be an absolute path");
    };
    let substitute: Vec<u16> = substitute.encode_utf16().collect();
    let print: Vec<u16> = print.encode_utf16().collect();
    let (substitute_bytes, print_bytes) = (2 * substitute.len(), 2 * print.len());
    let data_len = 8 + substitute_bytes + 2 + print_bytes + 2;
    if 8 + data_len > MAX_REPARSE_BUFFER_BYTES {
        return Err("junction target path is too long");
    }
    // Every length is bounded by the buffer limit above, so the u16 fields cannot truncate.
    let field = |value: usize| (value as u16).to_le_bytes();
    let mut buffer = Vec::with_capacity(8 + data_len);
    buffer.extend_from_slice(&REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer.extend_from_slice(&field(data_len));
    buffer.extend_from_slice(&[0, 0]);
    buffer.extend_from_slice(&field(0));
    buffer.extend_from_slice(&field(substitute_bytes));
    buffer.extend_from_slice(&field(substitute_bytes + 2));
    buffer.extend_from_slice(&field(print_bytes));
    for name in [&substitute, &print] {
        for unit in name {
            buffer.extend_from_slice(&unit.to_le_bytes());
        }
        buffer.extend_from_slice(&[0, 0]);
    }
    Ok(buffer)
}

/// Makes `link` a junction to `target`; junctions need no privilege, unlike symlinks.
#[cfg(windows)]
pub(crate) fn create_world_link(link: &Path, target: &Path) -> io::Result<()> {
    let absolute = std::path::absolute(target)?;
    let data = junction_reparse_data(&absolute.to_string_lossy()).map_err(io::Error::other)?;
    std::fs::create_dir(link)?;
    let linked = set_reparse_point(link, &data);
    if linked.is_err() {
        let _ = std::fs::remove_dir(link);
    }
    linked
}

#[cfg(windows)]
fn set_reparse_point(dir: &Path, data: &[u8]) -> io::Result<()> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::{
        Foundation::GENERIC_WRITE,
        Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT},
        System::{IO::DeviceIoControl, Ioctl::FSCTL_SET_REPARSE_POINT},
    };
    let directory = std::fs::OpenOptions::new()
        .access_mode(GENERIC_WRITE)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(dir)?;
    let length = u32::try_from(data.len()).map_err(io::Error::other)?;
    let mut returned = 0u32;
    // SAFETY: the handle stays open for the call and `data` outlives it; no output buffer or
    // overlapped structure is passed.
    let done = unsafe {
        DeviceIoControl(
            directory.as_raw_handle(),
            FSCTL_SET_REPARSE_POINT,
            data.as_ptr().cast(),
            length,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if done == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Makes `link` a symlink to `target`.
#[cfg(unix)]
pub(crate) fn create_world_link(link: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(test)]
#[path = "junction_tests.rs"]
mod tests;
/// Browsers have no file system to link worlds in.
#[cfg(not(any(unix, windows)))]
pub(crate) fn create_world_link(_link: &Path, _target: &Path) -> io::Result<()> {
    Err(io::ErrorKind::Unsupported.into())
}
