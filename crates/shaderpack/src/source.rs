//! Pack text and asset reads from folders, ZIP archives or in-memory files.

use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

/// Why a pack could not be opened.
#[derive(Debug)]
pub enum PackError {
    Io {
        path: PathBuf,
        error: io::Error,
    },
    Zip {
        path: PathBuf,
        error: zip::result::ZipError,
    },
    /// A pack exceeds the bounded ingestion policy.
    Limit {
        path: PathBuf,
        resource: &'static str,
    },
    /// Neither the path nor its top-level folder holds a `shaders/` directory.
    NotAShaderPack(PathBuf),
}

impl fmt::Display for PackError {
    /// Formats the pack path and the reason it could not be opened.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::Io { path, error } => write!(f, "{}: {error}", path.display()),
            PackError::Zip { path, error } => write!(f, "{}: {error}", path.display()),
            PackError::Limit { path, resource } => {
                write!(f, "{}: pack {resource} limit exceeded", path.display())
            }
            PackError::NotAShaderPack(path) => {
                write!(f, "{}: no shaders/ folder found", path.display())
            }
        }
    }
}

impl std::error::Error for PackError {}

/// Where the pack was loaded from, for reading binary assets (custom textures) later.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PackOrigin {
    /// The pack's canonical `shaders/` directory.
    Directory(PathBuf),
    /// The canonical archive path and the entry prefix ending in `shaders/`.
    Zip { archive: PathBuf, prefix: String },
    /// Files supplied in memory (tests and tools).
    Memory,
}

/// Extensions never read as text: images, audio, fonts and archives.
const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "tga", "bmp", "gif", "dat", "bin", "raw", "ktx", "dds", "hdr", "exr",
    "ogg", "wav", "mp3", "ttf", "otf", "zip", "jar", "class", "psd", "webp",
];

/// Maximum bytes read from one shader-pack text file.
pub(crate) const MAX_TEXT_FILE_BYTES: usize = 8 * 1024 * 1024;

/// Maximum expanded properties text retained from one input file.
pub(crate) const MAX_EXPANDED_PROPERTIES_BYTES: usize = 4 * MAX_TEXT_FILE_BYTES;

/// Bounds eager pack ingestion.
#[derive(Clone, Copy)]
struct LoadLimits {
    file_bytes: usize,
    total_bytes: usize,
    entries: usize,
    archive_bytes: usize,
}

impl LoadLimits {
    const DEFAULT: Self = Self {
        file_bytes: MAX_TEXT_FILE_BYTES,
        total_bytes: 64 * 1024 * 1024,
        entries: 16384,
        archive_bytes: 256 * 1024 * 1024,
    };
}

/// The text files under a pack's `shaders/` folder, keyed by `shaders/`-relative path with
/// forward slashes. Text is loaded eagerly and can be shared across worker threads.
#[derive(Clone, Debug)]
pub struct PackSource {
    origin: PackOrigin,
    files: BTreeMap<String, String>,
}

impl PackSource {
    /// Opens a pack folder (containing `shaders/`, or the `shaders/` folder itself) or a zip
    /// whose root or single top-level folder contains `shaders/`. Directory symlinks are
    /// skipped; origins are canonical so deferred asset reads survive working-directory changes.
    pub fn open(path: &Path) -> Result<PackSource, PackError> {
        if path.is_dir() {
            Self::open_dir(path, LoadLimits::DEFAULT)
        } else {
            Self::open_zip(path, LoadLimits::DEFAULT)
        }
    }

    /// Builds a source from in-memory files (`shaders/`-relative path → text).
    pub fn from_files<I, K, V>(files: I) -> PackSource
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        PackSource {
            origin: PackOrigin::Memory,
            files: files
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        }
    }

    fn open_dir(path: &Path, limits: LoadLimits) -> Result<PackSource, PackError> {
        let io_err = |path: &Path| {
            let path = path.to_path_buf();
            move |error| PackError::Io { path, error }
        };
        let directory = path.canonicalize().map_err(io_err(path))?;
        let shaders = directory.join("shaders");
        let root = if shaders.is_dir() {
            shaders.canonicalize().map_err(io_err(&shaders))?
        } else if directory.file_name().is_some_and(|n| n == "shaders")
            || path.file_name().is_some_and(|n| n == "shaders")
        {
            // A caller's `shaders` name also identifies a root reached through a symlink.
            directory
        } else {
            return Err(PackError::NotAShaderPack(path.to_path_buf()));
        };
        let mut budget = Budget::new(limits);
        let mut files = BTreeMap::new();
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).map_err(io_err(&dir))? {
                let entry = entry.map_err(io_err(&dir))?;
                let p = entry.path();
                budget.entry(&p)?;
                // `file_type` does not follow links, so symlinks are neither dirs nor files.
                let kind = entry.file_type().map_err(io_err(&p))?;
                if kind.is_dir() {
                    stack.push(p);
                    continue;
                }
                if !kind.is_file() {
                    continue;
                }
                let rel = relative_key(p.strip_prefix(&root).unwrap_or(&p)).map_err(io_err(&p))?;
                if is_binary_name(&rel) {
                    continue;
                }
                let file = std::fs::File::open(&p).map_err(io_err(&p))?;
                if let Some(text) = budget.text(file, &p)? {
                    files.insert(rel, text);
                }
            }
        }
        Ok(PackSource {
            origin: PackOrigin::Directory(root),
            files,
        })
    }

    fn open_zip(path: &Path, limits: LoadLimits) -> Result<PackSource, PackError> {
        let archive_path = path.canonicalize().map_err(|error| PackError::Io {
            path: path.to_path_buf(),
            error,
        })?;
        let path = archive_path.as_path();
        let mut archive = open_archive(path, limits)?;
        let prefix = shaders_prefix(archive.file_names())
            .ok_or_else(|| PackError::NotAShaderPack(path.to_path_buf()))?;
        let mut budget = Budget::new(limits);
        let mut files = BTreeMap::new();
        for i in 0..archive.len() {
            // Filter by name first so unread entries never need a decryptor or codec.
            let Some(rel) = archive
                .name_for_index(i)
                .and_then(|name| name.strip_prefix(prefix.as_str()))
                .filter(|rel| {
                    !rel.is_empty()
                        && !rel.ends_with('/')
                        && !is_binary_name(rel)
                        && !rel.split('/').any(|seg| seg == "..")
                })
                .map(str::to_string)
            else {
                continue;
            };
            let mut entry = archive.by_index(i).map_err(|error| PackError::Zip {
                path: path.to_path_buf(),
                error,
            })?;
            if entry.is_dir() || entry.is_symlink() {
                continue;
            }
            if let Some(text) = budget.text(&mut entry, &path.join(&rel))? {
                files.insert(rel, text);
            }
        }
        Ok(PackSource {
            origin: PackOrigin::Zip {
                archive: archive_path,
                prefix,
            },
            files,
        })
    }

    /// Where the pack came from.
    pub fn origin(&self) -> &PackOrigin {
        &self.origin
    }

    /// Text of a `shaders/`-relative file.
    pub fn text(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(String::as_str)
    }

    /// Every loaded text file, sorted by path.
    pub fn files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files.iter().map(|(k, v)| (k.as_ref(), v.as_str()))
    }

    /// Reads any `shaders/`-relative file as bytes, including binary assets. Directory
    /// targets must resolve inside the root at the time of the check.
    pub fn read_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        if Path::new(path)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "asset path must stay beneath shaders/",
            ));
        }
        let limit = LoadLimits::DEFAULT.total_bytes;
        match &self.origin {
            PackOrigin::Directory(root) => {
                let resolved = root.join(path).canonicalize()?;
                if !resolved.starts_with(root) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "asset path resolves outside shaders/",
                    ));
                }
                read_bounded(std::fs::File::open(resolved)?, limit)
            }
            PackOrigin::Zip { archive, prefix } => {
                let mut zip =
                    open_archive(archive, LoadLimits::DEFAULT).map_err(io::Error::other)?;
                let entry = zip
                    .by_name(&format!("{prefix}{path}"))
                    .map_err(io::Error::other)?;
                read_bounded(entry, limit)
            }
            PackOrigin::Memory => self
                .files
                .get(path)
                .map(|text| text.as_bytes().to_vec())
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.to_string())),
        }
    }
}

/// Joins relative components with `/`; non-Unicode names are rejected rather than mangled.
fn relative_key(path: &Path) -> io::Result<String> {
    let parts = path
        .components()
        .map(|component| {
            component.as_os_str().to_str().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "source path must be valid UTF-8",
                )
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    Ok(parts.join("/"))
}

/// The entry prefix ending in `shaders/` at the archive root or under one top-level folder.
fn shaders_prefix<'a>(names: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut best: Option<String> = None;
    for name in names {
        let parts: Vec<&str> = name.split('/').collect();
        let prefix = if parts.len() > 1 && parts[0] == "shaders" {
            "shaders/".to_string()
        } else if parts.len() > 2 && parts[1] == "shaders" {
            format!("{}/shaders/", parts[0])
        } else {
            continue;
        };
        if best.as_ref().is_none_or(|b| prefix.len() < b.len()) {
            best = Some(prefix);
        }
    }
    best
}

/// Recognises binary asset extensions without regard to case.
fn is_binary_name(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, ext)| BINARY_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Opens an archive no larger than the archive limit whose footer, checked before parsing,
/// declares at most the entry limit and exactly the entries the parser indexes.
fn open_archive(
    path: &Path,
    limits: LoadLimits,
) -> Result<zip::ZipArchive<io::Cursor<Vec<u8>>>, PackError> {
    let invalid = |error| PackError::Io {
        path: path.to_path_buf(),
        error: io::Error::new(io::ErrorKind::InvalidData, error),
    };
    let file = std::fs::File::open(path).map_err(|error| PackError::Io {
        path: path.to_path_buf(),
        error,
    })?;
    let mut bytes =
        read_bounded(file, limits.archive_bytes).map_err(|error| match error.kind() {
            io::ErrorKind::FileTooLarge => limit_error(path, "archive size"),
            _ => PackError::Io {
                path: path.to_path_buf(),
                error,
            },
        })?;
    let entries =
        zip_footer::prepare_zip_bytes(&mut bytes, limits.entries).map_err(|error| match error {
            zip_footer::FooterError::TooManyEntries => limit_error(path, "entry"),
            error => invalid(error.to_string()),
        })?;
    let archive = zip::ZipArchive::new(io::Cursor::new(bytes)).map_err(|error| PackError::Zip {
        path: path.to_path_buf(),
        error,
    })?;
    if archive.len() != entries {
        return Err(invalid("ZIP entry names repeat".to_string()));
    }
    Ok(archive)
}

/// Entry and byte counters shared by every file one pack ingests, including discarded text.
struct Budget {
    limits: LoadLimits,
    entries: usize,
    bytes: usize,
}

impl Budget {
    fn new(limits: LoadLimits) -> Self {
        Self {
            limits,
            entries: 0,
            bytes: 0,
        }
    }

    fn entry(&mut self, path: &Path) -> Result<(), PackError> {
        self.entries += 1;
        if self.entries > self.limits.entries {
            return Err(limit_error(path, "entry"));
        }
        Ok(())
    }

    /// Reads one file within the per-file and remaining total limits; `None` for binary data
    /// (a NUL byte in the first 8 KiB). Text loses its BOM and invalid UTF-8 is replaced.
    fn text(&mut self, reader: impl Read, path: &Path) -> Result<Option<String>, PackError> {
        let limit = self
            .limits
            .file_bytes
            .min(self.limits.total_bytes - self.bytes);
        let bytes = read_bounded(reader, limit).map_err(|error| match error.kind() {
            io::ErrorKind::FileTooLarge => limit_error(path, "text byte"),
            _ => PackError::Io {
                path: path.to_path_buf(),
                error,
            },
        })?;
        self.bytes += bytes.len();
        if bytes.iter().take(8192).any(|&b| b == 0) {
            return Ok(None);
        }
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        Ok(Some(String::from_utf8_lossy(bytes).into_owned()))
    }
}

fn limit_error(path: &Path, resource: &'static str) -> PackError {
    PackError::Limit {
        path: path.to_path_buf(),
        resource,
    }
}

/// Reads at most `limit` bytes, failing once the reader has more.
fn read_bounded(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut out)?;
    if out.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::FileTooLarge,
            "pack byte limit exceeded",
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
