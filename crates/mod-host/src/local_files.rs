//! Regular-file reads from selected directories, without following package links.

use std::{
    fs::File,
    io::{self, Read},
    path::{Component, Path},
};

mod observation;
pub use observation::{CONTENT_REVALIDATION_INTERVAL, ObservationCache, Snapshot};
pub(crate) use observation::{ContentDeadline, Identity};

/// Reads a selected regular file with a byte ceiling and refuses a linked final entry.
pub fn read(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    open(path)?.read(limit)
}

/// Resolves directory aliases, then opens the leaf without following links.
fn open(path: &Path) -> io::Result<Opened> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("source has no file name"))?;
    Directory::open(&parent)?.open_file(Path::new(name))
}

/// An open directory that anchors all reads of one package snapshot.
pub(crate) struct Directory {
    file: File,
    #[cfg(windows)]
    path: std::path::PathBuf,
}

impl Directory {
    /// Opens the selected directory without following a linked final entry.
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let path: std::path::PathBuf = path.components().collect();
        #[cfg(unix)]
        let file = File::from(rustix::fs::open(
            &path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )?);
        #[cfg(windows)]
        let file = open_windows(&path, true)?;
        #[cfg(not(any(unix, windows)))]
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "local mod files unavailable",
        ));
        #[cfg(any(unix, windows))]
        Ok(Self {
            file,
            #[cfg(windows)]
            path,
        })
    }

    /// Reads one normal relative path through directory handles and bounds growing files.
    pub(crate) fn read(&self, path: &Path, limit: usize) -> io::Result<Vec<u8>> {
        self.open_file(path)?.read(limit)
    }

    /// Keeps traversed Windows parents fixed until the leaf observation finishes.
    pub(crate) fn open_file(&self, path: &Path) -> io::Result<Opened> {
        let components = path
            .components()
            .map(|component| match component {
                Component::Normal(name) => Ok(name),
                _ => Err(io::Error::other(
                    "source path must stay inside its directory",
                )),
            })
            .collect::<io::Result<Vec<_>>>()?;
        if components.is_empty() {
            return Err(io::Error::other("source path is empty"));
        }
        let mut opened = self.file.try_clone()?;
        #[cfg(windows)]
        let mut current = self.path.clone();
        // Windows holds every parent without delete sharing until the read finishes.
        #[cfg(windows)]
        let mut parents = Vec::new();
        for (index, name) in components.iter().enumerate() {
            let directory = index + 1 < components.len();
            #[cfg(unix)]
            {
                use rustix::fs::{Mode, OFlags, openat};
                let flags = OFlags::RDONLY
                    | OFlags::NOFOLLOW
                    | OFlags::CLOEXEC
                    | OFlags::NONBLOCK
                    | if directory {
                        OFlags::DIRECTORY
                    } else {
                        OFlags::empty()
                    };
                opened = File::from(openat(&opened, Path::new(name), flags, Mode::empty())?);
            }
            #[cfg(windows)]
            {
                current.push(name);
                parents.push(opened);
                opened = open_windows(&current, directory)?;
            }
            #[cfg(not(any(unix, windows)))]
            {
                let _ = (directory, name);
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "local mod files unavailable",
                ));
            }
        }
        if !opened.metadata()?.is_file() {
            return Err(io::Error::other("source is not a regular file"));
        }
        Ok(Opened {
            file: opened,
            #[cfg(windows)]
            _parents: parents,
        })
    }
}

/// A selected leaf and any parent handles needed to keep its traversal fixed.
pub(crate) struct Opened {
    pub(crate) file: File,
    #[cfg(windows)]
    _parents: Vec<File>,
}

impl Opened {
    /// Reads the already selected regular file within the existing admission ceiling.
    pub(crate) fn read(&mut self, limit: usize) -> io::Result<Vec<u8>> {
        if self.file.metadata()?.len() > limit as u64 {
            return Err(io::Error::other("source exceeds byte limit"));
        }
        let ceiling = u64::try_from(limit)
            .ok()
            .and_then(|limit| limit.checked_add(1))
            .ok_or_else(|| io::Error::other("invalid source byte limit"))?;
        let mut bytes = Vec::new();
        (&mut self.file).take(ceiling).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(io::Error::other("source exceeds byte limit"));
        }
        Ok(bytes)
    }
}

/// Opens a Windows entry itself; directories stay fixed while leaf files allow replacement.
#[cfg(windows)]
fn open_windows(path: &Path, directory: bool) -> io::Result<File> {
    use std::os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK, GetFileType,
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(
            FILE_SHARE_READ | FILE_SHARE_WRITE | if directory { 0 } else { FILE_SHARE_DELETE },
        )
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT
                | if directory {
                    FILE_FLAG_BACKUP_SEMANTICS
                } else {
                    0
                },
        )
        .open(path)?;
    // SAFETY: the file owns a live handle for the duration of this query.
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK {
        return Err(io::Error::other("source is not a regular disk entry"));
    }
    let metadata = file.metadata()?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        }
    {
        return Err(io::Error::other("source must be a regular unlinked entry"));
    }
    Ok(file)
}

#[cfg(test)]
mod tests;
