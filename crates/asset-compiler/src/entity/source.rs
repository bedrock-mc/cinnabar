use std::{
    fs::{File, OpenOptions},
    io::{self, Read},
    path::{Component, Path, PathBuf},
};

use assets::{AssetError, MAX_ENTITY_SOURCE_BYTES};

use super::invalid;

pub(super) fn read_bounded_source(root: &Path, path: &Path) -> Result<Vec<u8>, AssetError> {
    let file = open_source_handle(root, path).map_err(|source| AssetError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let length = file
        .metadata()
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if length == 0 || length > MAX_ENTITY_SOURCE_BYTES as u64 {
        return Err(invalid("entity asset source size exceeds bound"));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_ENTITY_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if bytes.len() > MAX_ENTITY_SOURCE_BYTES {
        return Err(invalid("entity asset source size exceeds bound"));
    }
    Ok(bytes)
}

fn relative_components<'a>(root: &Path, path: &'a Path) -> io::Result<Vec<&'a std::ffi::OsStr>> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::Error::new(io::ErrorKind::PermissionDenied, "source escaped root"))?;
    let mut components = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => components.push(value),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "source path is not canonical relative data",
                ));
            }
        }
    }
    if components.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source path is empty",
        ));
    }
    Ok(components)
}

#[cfg(windows)]
fn open_source_handle(root: &Path, path: &Path) -> io::Result<File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let components = relative_components(root, path)?;
    let root_handle = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(root)?;
    let root_metadata = root_handle.metadata()?;
    if !root_metadata.is_dir()
        || root_metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "entity root handle is a reparse point or not a directory",
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "entity source handle is a reparse point or not a file",
        ));
    }
    let root_final = windows_final_path(&root_handle)?;
    let file_final = windows_final_path(&file)?;
    let expected = components
        .into_iter()
        .fold(root_final.clone(), |path, component| path.join(component));
    if file_final != expected || !file_final.starts_with(&root_final) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "entity source handle was redirected outside its enumerated path",
        ));
    }
    Ok(file)
}

#[cfg(windows)]
fn windows_final_path(file: &File) -> io::Result<PathBuf> {
    use std::{
        ffi::OsString,
        os::windows::{ffi::OsStringExt, io::AsRawHandle},
    };

    const MAX_FINAL_PATH_UNITS: usize = 32_768;
    let mut buffer = vec![0_u16; 512];
    loop {
        let capacity = u32::try_from(buffer.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "final path too long"))?;
        // SAFETY: `file` owns a live handle and the buffer has `capacity` writable units.
        let written = unsafe {
            GetFinalPathNameByHandleW(file.as_raw_handle(), buffer.as_mut_ptr(), capacity, 0)
        };
        if written == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = written as usize;
        if length < buffer.len() {
            buffer.truncate(length);
            return Ok(PathBuf::from(OsString::from_wide(&buffer)));
        }
        if length >= MAX_FINAL_PATH_UNITS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "final path too long",
            ));
        }
        buffer.resize(length + 1, 0);
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetFinalPathNameByHandleW(
        file: std::os::windows::io::RawHandle,
        path: *mut u16,
        path_units: u32,
        flags: u32,
    ) -> u32;
}

#[cfg(unix)]
fn open_source_handle(root: &Path, path: &Path) -> io::Result<File> {
    use std::{
        ffi::CString,
        os::unix::{
            ffi::OsStrExt,
            fs::OpenOptionsExt,
            io::{AsRawFd, FromRawFd},
        },
    };

    let components = relative_components(root, path)?;
    let mut current = OpenOptions::new()
        .read(true)
        .custom_flags(unix_flags::NOFOLLOW | unix_flags::DIRECTORY | unix_flags::CLOEXEC)
        .open(root)?;
    for (index, component) in components.iter().enumerate() {
        let name = CString::new(component.as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source contains NUL"))?;
        let directory = index + 1 < components.len();
        let flags = unix_flags::NOFOLLOW
            | unix_flags::CLOEXEC
            | if directory { unix_flags::DIRECTORY } else { 0 };
        // SAFETY: `current` is live and `name` is NUL terminated; no create flag is used.
        let descriptor = unsafe { openat(current.as_raw_fd(), name.as_ptr(), flags) };
        if descriptor < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: successful `openat` returns one newly owned descriptor.
        let opened = unsafe { File::from_raw_fd(descriptor) };
        if directory && !opened.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "source parent is not a directory",
            ));
        }
        current = opened;
    }
    if !current.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "source is not a file",
        ));
    }
    Ok(current)
}

#[cfg(unix)]
unsafe extern "C" {
    fn openat(
        directory: std::os::raw::c_int,
        path: *const std::os::raw::c_char,
        flags: std::os::raw::c_int,
    ) -> std::os::raw::c_int;
}

#[cfg(all(unix, any(target_os = "linux", target_os = "android")))]
mod unix_flags {
    pub const DIRECTORY: i32 = 0x1_0000;
    pub const NOFOLLOW: i32 = 0x2_0000;
    pub const CLOEXEC: i32 = 0x8_0000;
}

#[cfg(all(unix, any(target_os = "macos", target_os = "ios")))]
mod unix_flags {
    pub const DIRECTORY: i32 = 0x10_0000;
    pub const NOFOLLOW: i32 = 0x100;
    pub const CLOEXEC: i32 = 0x100_0000;
}

#[cfg(all(
    unix,
    not(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios"
    ))
))]
compile_error!("secure entity source opening requires reviewed openat flags for this Unix target");

#[cfg(not(any(unix, windows)))]
fn open_source_handle(_root: &Path, _path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure entity source opening is unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::read_bounded_source;

    #[test]
    fn rejects_parent_replaced_by_external_link_after_enumeration() {
        let root = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        let parent = root.path().join("animations");
        fs::create_dir(&parent).expect("parent");
        let enumerated = parent.join("actor.json");
        fs::write(&enumerated, b"original").expect("enumerated source");

        fs::remove_dir_all(&parent).expect("remove enumerated parent");
        fs::write(outside.path().join("actor.json"), b"redirected").expect("outside source");
        if let Err(error) = link_directory(outside.path(), &parent) {
            eprintln!("skipping link swap case: {error}");
            return;
        }

        assert!(read_bounded_source(root.path(), &enumerated).is_err());
    }

    #[cfg(unix)]
    fn link_directory(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn link_directory(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
        use std::process::Command;

        let status = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "mklink /J failed with {status}"
            )))
        }
    }
}
