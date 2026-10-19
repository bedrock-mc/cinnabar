//! Pack text and asset reads from folders, ZIP archives or in-memory files.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

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
            PackError::Limit { path, resource } => write!(
                f,
                "{}: pack ingestion {resource} budget exceeded",
                path.display()
            ),
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
    /// The pack's `shaders/` directory.
    Directory(PathBuf),
    /// The canonical archive path and the entry prefix of its `shaders/` folder (`""` or `"<top>/"`
    /// followed by `shaders/`).
    Zip { archive: PathBuf, prefix: String },
    /// Files supplied in memory (tests and tools).
    Memory,
}

/// Extensions never read as text: images, audio, fonts and archives.
const BINARY_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "tga", "bmp", "gif", "dat", "bin", "raw", "ktx", "dds", "hdr", "exr",
    "ogg", "wav", "mp3", "ttf", "otf", "zip", "jar", "class", "psd", "webp",
];

/// Maximum bytes in one decoded text file or the cumulative pack path budget.
const MAX_INPUT_TEXT_BYTES: usize = 8 * 1024 * 1024;

/// Maximum read and seek operations across archive preflight, indexing and payload reads.
const MAX_ARCHIVE_WORK_ITEMS: usize = 262144;

/// Bounds eager pack ingestion.
#[derive(Clone, Copy)]
struct LoadLimits {
    file_bytes: usize,
    total_bytes: usize,
    entries: usize,
    path_bytes: usize,
    archive_bytes: usize,
}

impl LoadLimits {
    const DEFAULT: Self = Self {
        file_bytes: MAX_INPUT_TEXT_BYTES,
        total_bytes: 64 * 1024 * 1024,
        entries: 16384,
        path_bytes: MAX_INPUT_TEXT_BYTES,
        archive_bytes: 256 * 1024 * 1024,
    };
}

/// The text files under a pack's `shaders/` folder, keyed by `shaders/`-relative path with
/// forward slashes. Text is loaded eagerly and can be shared across worker threads.
#[derive(Clone, Debug)]
pub struct PackSource {
    origin: PackOrigin,
    files: BTreeMap<Arc<str>, String>,
}

impl PackSource {
    /// Opens a pack folder (containing `shaders/`, or the `shaders/` folder itself) or a zip
    /// whose root or single top-level folder contains `shaders/`. Text, inflated bytes,
    /// entries and paths share finite ingestion budgets. Directory symlinks are skipped.
    /// Archives use an unambiguous standard ZIP end record with no prepended or trailing
    /// payload. Directory source paths must have valid Unicode components without literal
    /// backslashes. Directory and archive origins are canonical absolute paths.
    pub fn open(path: &Path) -> Result<PackSource, PackError> {
        if path.is_dir() {
            Self::open_dir(path)
        } else {
            Self::open_zip(path)
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
                .map(|(k, v)| (Arc::from(k.into()), v.into()))
                .collect(),
        }
    }

    /// Loads decodable text files beneath a pack's shaders directory.
    fn open_dir(path: &Path) -> Result<PackSource, PackError> {
        Self::open_dir_limited(path, LoadLimits::DEFAULT)
    }

    /// Loads a directory within the supplied ingestion budget.
    fn open_dir_limited(path: &Path, limits: LoadLimits) -> Result<PackSource, PackError> {
        let root = if path.join("shaders").is_dir() {
            path.join("shaders")
        } else if path.file_name().is_some_and(|n| n == "shaders") {
            path.to_path_buf()
        } else {
            return Err(PackError::NotAShaderPack(path.to_path_buf()));
        };
        let root = root.canonicalize().map_err(|error| PackError::Io {
            path: root.clone(),
            error,
        })?;
        let mut files = BTreeMap::new();
        let mut budget = LoadBudget::new(limits);
        let mut stack = vec![root.clone()];
        while let Some(dir) = stack.pop() {
            let entries = std::fs::read_dir(&dir).map_err(|error| PackError::Io {
                path: dir.clone(),
                error,
            })?;
            for entry in entries {
                let entry = entry.map_err(|error| PackError::Io {
                    path: dir.clone(),
                    error,
                })?;
                let p = entry.path();
                budget.entry(&p, p.as_os_str().len())?;
                let kind = entry.file_type().map_err(|error| PackError::Io {
                    path: p.clone(),
                    error,
                })?;
                // Links are deliberately excluded, including acyclic directory graphs.
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    stack.push(p);
                    continue;
                }
                if !kind.is_file() {
                    continue;
                }
                let Ok(rel) = p.strip_prefix(&root) else {
                    continue;
                };
                let rel = portable_relative_path(rel).map_err(|error| PackError::Io {
                    path: p.clone(),
                    error,
                })?;
                if is_binary_name(&rel) {
                    continue;
                }
                let file = std::fs::File::open(&p).map_err(|error| PackError::Io {
                    path: p.clone(),
                    error,
                })?;
                if let Some(text) = budget.text(file, &p)? {
                    files.insert(Arc::from(rel), text);
                }
            }
        }
        Ok(PackSource {
            origin: PackOrigin::Directory(root),
            files,
        })
    }

    /// Loads decodable text entries under a zip pack's shaders prefix.
    fn open_zip(path: &Path) -> Result<PackSource, PackError> {
        Self::open_zip_limited(path, LoadLimits::DEFAULT)
    }

    /// Loads an archive within the supplied ingestion budget.
    fn open_zip_limited(path: &Path, limits: LoadLimits) -> Result<PackSource, PackError> {
        let archive_path = path.canonicalize().map_err(|error| PackError::Io {
            path: path.to_path_buf(),
            error,
        })?;
        let path = archive_path.as_path();
        let zip_err = |error| PackError::Zip {
            path: path.to_path_buf(),
            error,
        };
        let mut archive = open_archive(path, limits)?;
        let mut budget = LoadBudget::new(limits);
        let prefix = shaders_prefix(archive.file_names())
            .ok_or_else(|| PackError::NotAShaderPack(path.to_path_buf()))?;
        let mut files = BTreeMap::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(zip_err)?;
            budget.entry(path, entry.name().len())?;
            if entry.is_dir() || entry.is_symlink() {
                continue;
            }
            let Some(rel) = entry.name().strip_prefix(prefix.as_str()) else {
                continue;
            };
            let rel = rel.to_string();
            if is_binary_name(&rel) || rel.split('/').any(|seg| seg == "..") {
                continue;
            }
            if let Some(text) = budget.text(&mut entry, &path.join(&rel))? {
                files.insert(Arc::from(rel), text);
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
        self.shared_file(path).map(|(_, text)| text)
    }

    /// Every loaded text file, sorted by path.
    pub fn files(&self) -> impl Iterator<Item = (&str, &str)> {
        self.files.iter().map(|(k, v)| (k.as_ref(), v.as_str()))
    }

    /// Looks up text and its canonical shared metadata path without copying either.
    fn shared_file(&self, path: &str) -> Option<(&Arc<str>, &str)> {
        self.files
            .get_key_value(path)
            .map(|(path, text)| (path, text.as_str()))
    }

    /// Reads any `shaders/`-relative file as bytes, including binary assets.
    /// Directory targets must resolve inside the root; concurrent filesystem changes
    /// during resolution and opening are outside this check's guarantee.
    pub fn read_bytes(&self, path: &str) -> io::Result<Vec<u8>> {
        if Path::new(path).components().any(|component| {
            !matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "asset path must stay beneath shaders/",
            ));
        }
        match &self.origin {
            PackOrigin::Directory(root) => {
                let resolved = root.join(path).canonicalize()?;
                if !resolved.starts_with(root) {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "asset path resolves outside shaders/",
                    ));
                }
                read_bounded(
                    std::fs::File::open(resolved)?,
                    LoadLimits::DEFAULT.total_bytes,
                )
            }
            PackOrigin::Zip { archive, prefix } => {
                let mut zip =
                    open_archive(archive, LoadLimits::DEFAULT).map_err(io::Error::other)?;
                let mut entry = zip
                    .by_name(&format!("{prefix}{path}"))
                    .map_err(io::Error::other)?;
                read_bounded(&mut entry, LoadLimits::DEFAULT.total_bytes)
            }
            PackOrigin::Memory => self
                .files
                .get(path)
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, path.to_string()))
                .and_then(|text| read_bounded(text.as_bytes(), LoadLimits::DEFAULT.total_bytes)),
        }
    }
}

/// Joins native relative components losslessly into an unambiguous portable source key.
fn portable_relative_path(path: &Path) -> io::Result<String> {
    let mut relative = String::new();
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "source path must be relative",
            ));
        };
        let name = name.to_str().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "source path must be valid UTF-8",
            )
        })?;
        if name.contains('\\') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "source path component contains a backslash",
            ));
        }
        if !relative.is_empty() {
            relative.push('/');
        }
        relative.push_str(name);
    }
    Ok(relative)
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

/// Stateful counters shared by every eagerly loaded file, including discarded text.
struct LoadBudget {
    limits: LoadLimits,
    entries: usize,
    paths: usize,
    inflated: usize,
    decoded: usize,
}

impl LoadBudget {
    /// Starts one pack's counters.
    fn new(limits: LoadLimits) -> Self {
        Self {
            limits,
            entries: 0,
            paths: 0,
            inflated: 0,
            decoded: 0,
        }
    }

    /// Charges entry and path work before traversal or name retention.
    fn entry(&mut self, path: &Path, bytes: usize) -> Result<(), PackError> {
        charge(&mut self.entries, 1, self.limits.entries, path, "entry")?;
        charge(&mut self.paths, bytes, self.limits.path_bytes, path, "path")
    }

    /// Reads at most the remaining actual inflated budget, then checks decoded growth.
    fn text(&mut self, reader: impl Read, path: &Path) -> Result<Option<String>, PackError> {
        let remaining = self.limits.total_bytes - self.inflated;
        let limit = remaining.min(self.limits.file_bytes);
        let bytes = read_bounded(reader, limit).map_err(|error| PackError::Io {
            path: path.to_path_buf(),
            error,
        })?;
        self.inflated += bytes.len();
        if bytes.iter().take(8192).any(|b| *b == 0) {
            return Ok(None);
        }
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        let mut rest = bytes;
        let mut size = 0;
        while !rest.is_empty() {
            match std::str::from_utf8(rest) {
                Ok(_) => {
                    size += rest.len();
                    break;
                }
                Err(error) => {
                    size += error.valid_up_to() + '\u{fffd}'.len_utf8();
                    rest = &rest[error.valid_up_to()
                        + error
                            .error_len()
                            .unwrap_or(rest.len() - error.valid_up_to())..];
                }
            }
            if size > self.limits.file_bytes {
                return Err(limit_error(path, "decoded file"));
            }
        }
        if size > self.limits.file_bytes {
            return Err(limit_error(path, "decoded file"));
        }
        charge(
            &mut self.decoded,
            size,
            self.limits.total_bytes,
            path,
            "decoded text",
        )?;
        Ok(Some(String::from_utf8_lossy(bytes).into_owned()))
    }
}

/// Rejects a counter overflow before allocating or retaining the corresponding data.
fn charge(
    total: &mut usize,
    amount: usize,
    limit: usize,
    path: &Path,
    resource: &'static str,
) -> Result<(), PackError> {
    if amount > limit.saturating_sub(*total) {
        return Err(limit_error(path, resource));
    }
    *total += amount;
    Ok(())
}

/// Creates a located ingestion limit error.
fn limit_error(path: &Path, resource: &'static str) -> PackError {
    PackError::Limit {
        path: path.to_path_buf(),
        resource,
    }
}

/// Reads actual bytes with a fixed scratch buffer and geometric capacity growth capped by limit.
fn read_bounded(mut reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut scratch = [0; 8192];
    loop {
        let request = scratch
            .len()
            .min(limit.saturating_sub(out.len()).saturating_add(1));
        let count = reader.read(&mut scratch[..request])?;
        if count == 0 {
            return Ok(out);
        }
        let required = out
            .len()
            .checked_add(count)
            .filter(|&size| size <= limit)
            .ok_or_else(|| io::Error::other("pack ingestion byte budget exceeded"))?;
        if required > out.capacity() {
            let capacity = out
                .capacity()
                .checked_mul(2)
                .unwrap_or(limit)
                .max(required)
                .min(limit);
            out.try_reserve_exact(capacity - out.len())
                .map_err(io::Error::other)?;
        }
        out.extend_from_slice(&scratch[..count]);
    }
}

/// Opens a preflighted index through one cumulative read budget for the whole archive.
fn open_archive(
    path: &Path,
    limits: LoadLimits,
) -> Result<zip::ZipArchive<ZipIndexReader>, PackError> {
    let zip_err = |error| PackError::Zip {
        path: path.to_path_buf(),
        error,
    };
    let file = std::fs::File::open(path).map_err(|error| PackError::Io {
        path: path.to_path_buf(),
        error,
    })?;
    let index_check = Rc::new(Cell::new(IndexCheck::Inactive));
    let mut reader = ZipIndexReader {
        file,
        footer: 0,
        index_check: index_check.clone(),
        candidate: false,
        work_left: MAX_ARCHIVE_WORK_ITEMS,
        bytes_left: limits.archive_bytes,
    };
    reader.footer = check_zip_index(&mut reader, path, limits)?;
    index_check.set(IndexCheck::Active);
    let archive = zip::ZipArchive::with_config(
        zip::read::Config {
            archive_offset: zip::read::ArchiveOffset::Known(0),
        },
        reader,
    )
    .map_err(zip_err)?;
    index_check.set(IndexCheck::Inactive);
    Ok(archive)
}

/// Whether metadata parsing is confined to the one preflighted end record.
#[derive(Clone, Copy, PartialEq, Eq)]
enum IndexCheck {
    Inactive,
    Active,
    Rejected,
}

/// A rejected index remains an error even when the ZIP parser tries an older candidate.
const AMBIGUOUS_INDEX: &str = "ambiguous ZIP end record";

/// Bounds actual archive reads across indexing and repeated compressed payloads.
struct ZipIndexReader {
    file: std::fs::File,
    footer: u64,
    index_check: Rc<Cell<IndexCheck>>,
    candidate: bool,
    work_left: usize,
    bytes_left: usize,
}

impl ZipIndexReader {
    /// Prevents fallback from resuming after a metadata candidate escaped the checked index.
    fn ensure_index(&self) -> io::Result<()> {
        if self.index_check.get() == IndexCheck::Rejected {
            Err(io::Error::other(AMBIGUOUS_INDEX))
        } else {
            Ok(())
        }
    }

    /// Charges actual input before forwarding bytes to metadata or decompression consumers.
    fn read_counted(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.work_left = self
            .work_left
            .checked_sub(1)
            .ok_or_else(|| io::Error::other("ZIP read work budget exceeded"))?;
        let request = out.len().min(self.bytes_left.saturating_add(1));
        let count = self.file.read(&mut out[..request])?;
        if count > self.bytes_left {
            self.bytes_left = 0;
            self.work_left = 0;
            return Err(io::Error::other("ZIP compressed read budget exceeded"));
        }
        self.bytes_left -= count;
        Ok(count)
    }
}

impl Read for ZipIndexReader {
    /// Keeps alternative end-record probes from allocating an unbudgeted entry table.
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.ensure_index()?;
        if self.index_check.get() == IndexCheck::Active && self.candidate && out.len() == 22 {
            let position = self.file.stream_position()?;
            let mut magic = [0; 4];
            let count = self.read_counted(&mut magic)?;
            self.file.seek(SeekFrom::Start(position))?;
            if count == magic.len() && magic == *b"PK\x05\x06" && position != self.footer {
                self.index_check.set(IndexCheck::Rejected);
                return Err(io::Error::other(AMBIGUOUS_INDEX));
            }
        }
        self.candidate = false;
        self.read_counted(out)
    }
}

impl Seek for ZipIndexReader {
    /// Tracks explicit index probes independently of ordinary sequential field reads.
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.ensure_index()?;
        self.work_left = self
            .work_left
            .checked_sub(1)
            .ok_or_else(|| io::Error::other("ZIP seek work budget exceeded"))?;
        if !matches!(position, SeekFrom::Current(0)) {
            self.candidate = matches!(position, SeekFrom::Start(_));
        }
        self.file.seek(position)
    }
}

/// Checks the bounded standard ZIP index before the ZIP reader allocates its entry table.
/// ZIP64 and prepended/trailing payloads exceed this loader's container policy.
fn check_zip_index(
    file: &mut (impl Read + Seek),
    path: &Path,
    limits: LoadLimits,
) -> Result<u64, PackError> {
    let io_err = |error| PackError::Io {
        path: path.to_path_buf(),
        error,
    };
    let length = file.seek(SeekFrom::End(0)).map_err(io_err)?;
    if length > limits.archive_bytes as u64 {
        return Err(limit_error(path, "archive byte"));
    }
    let tail_len = length.min(22 + u16::MAX as u64) as usize;
    file.seek(SeekFrom::End(-(tail_len as i64)))
        .map_err(io_err)?;
    let mut tail = vec![0; tail_len];
    file.read_exact(&mut tail).map_err(io_err)?;
    let footer = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| {
            tail[i..].starts_with(b"PK\x05\x06")
                && i + 22 + u16::from_le_bytes([tail[i + 20], tail[i + 21]]) as usize == tail.len()
        })
        .ok_or_else(|| limit_error(path, "ZIP framing"))?;
    let short = |offset| u16::from_le_bytes([tail[footer + offset], tail[footer + offset + 1]]);
    let word = |offset| {
        u32::from_le_bytes(
            tail[footer + offset..footer + offset + 4]
                .try_into()
                .expect("fixed footer field"),
        )
    };
    let entries = short(10) as usize;
    let directory_len = word(12) as u64;
    let directory_start = word(16) as u64;
    let footer_start = length - tail_len as u64 + footer as u64;
    if short(4) != 0
        || short(6) != 0
        || short(8) as usize != entries
        || entries > limits.entries
        || entries == u16::MAX as usize
        || directory_start + directory_len != footer_start
    {
        return Err(limit_error(path, "ZIP index"));
    }
    let mut paths = 0;
    file.seek(SeekFrom::Start(directory_start))
        .map_err(io_err)?;
    for _ in 0..entries {
        let mut header = [0; 46];
        file.read_exact(&mut header).map_err(io_err)?;
        if !header.starts_with(b"PK\x01\x02") {
            return Err(limit_error(path, "ZIP framing"));
        }
        let len = |offset| u16::from_le_bytes([header[offset], header[offset + 1]]) as usize;
        charge(
            &mut paths,
            (len(28) + len(30) + len(32)).saturating_mul(3),
            limits.path_bytes,
            path,
            "index text",
        )?;
        let mut name = vec![0; len(28)];
        file.read_exact(&mut name).map_err(io_err)?;
        let flags = u16::from_le_bytes([header[8], header[9]]);
        if flags & (1 << 11) != 0 && std::str::from_utf8(&name).is_err() {
            return Err(io_err(io::Error::new(
                io::ErrorKind::InvalidData,
                "ZIP source path must be valid UTF-8",
            )));
        }
        file.seek(SeekFrom::Current((len(30) + len(32)) as i64))
            .map_err(io_err)?;
        if file.stream_position().map_err(io_err)? > footer_start {
            return Err(limit_error(path, "ZIP framing"));
        }
    }
    if file.stream_position().map_err(io_err)? != footer_start {
        return Err(limit_error(path, "ZIP framing"));
    }
    file.rewind().map_err(io_err)?;
    Ok(footer_start)
}

#[cfg(test)]
mod tests;
