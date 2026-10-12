//! Carrier file access. Native builds read carriers from disk. Browsers have no filesystem, so
//! the page compiles or fetches every carrier, then installs them with [`preload`] before the
//! client starts; loaders open them by the same paths either way and see the same errors. Once
//! startup has decoded them, [`release_preloaded`] frees the bytes no later load reads.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

/// Carriers held in memory, keyed by the path a loader opens.
#[derive(Clone, Debug, Default)]
pub struct MemoryCarriers {
    files: HashMap<PathBuf, Arc<[u8]>>,
    ranged: HashMap<PathBuf, RangedCarrier>,
    remote: HashMap<PathBuf, Arc<str>>,
}

/// A carrier a browser downloads in pieces as they are read: its first bytes, which hold its
/// index, are in memory, and the rest is fetched from `url` by byte range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RangedCarrier {
    pub prefix: Arc<[u8]>,
    pub url: Arc<str>,
}

impl MemoryCarriers {
    /// Adds or replaces the carrier at `path`.
    pub fn insert(&mut self, path: impl Into<PathBuf>, bytes: impl Into<Arc<[u8]>>) {
        self.files.insert(path.into(), bytes.into());
    }

    /// Adds or replaces a carrier read by byte range from `url`, whose first bytes are `prefix`.
    pub fn insert_ranged(
        &mut self,
        path: impl Into<PathBuf>,
        prefix: impl Into<Arc<[u8]>>,
        url: impl Into<Arc<str>>,
    ) {
        self.ranged.insert(
            path.into(),
            RangedCarrier {
                prefix: prefix.into(),
                url: url.into(),
            },
        );
    }

    /// Adds a carrier the client downloads whole from `url` only if it needs it, as a language a
    /// player may switch to; until then it opens as `NotFound`.
    pub fn insert_remote(&mut self, path: impl Into<PathBuf>, url: impl Into<Arc<str>>) {
        self.remote.insert(path.into(), url.into());
    }

    /// Drops every whole carrier `keep` rejects; ranged carriers stay.
    pub fn release(&mut self, keep: impl Fn(&Path) -> bool) {
        self.files.retain(|path, _| keep(path));
    }

    /// Opens the carrier at `path`; one never inserted or since released is `NotFound`, as a
    /// missing file is.
    pub fn open(&self, path: &Path) -> io::Result<CarrierFile> {
        let bytes = self.files.get(path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not in memory", path.display()),
            )
        })?;
        Ok(CarrierFile {
            len: bytes.len() as u64,
            source: Source::Memory(Cursor::new(Arc::clone(bytes))),
        })
    }
}

static PRELOADED: RwLock<Option<MemoryCarriers>> = RwLock::new(None);

/// Runs `read` over the preloaded carriers, or returns `None` on native builds.
fn with_preloaded<T>(read: impl FnOnce(&MemoryCarriers) -> T) -> Option<T> {
    PRELOADED
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(read)
}

/// Installs the carriers a browser serves from memory; only the first call takes effect, and it
/// must come before the client starts loading. Native builds keep reading the filesystem.
pub fn preload(carriers: MemoryCarriers) {
    PRELOADED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert(carriers);
}

/// Frees the preloaded bytes of every whole carrier `keep` rejects, for a client whose startup
/// has decoded them and that never reads them again; opening one afterwards is `NotFound`.
/// Ranged carriers stay. Native builds, which preload nothing, are unaffected.
pub fn release_preloaded(keep: impl Fn(&Path) -> bool) {
    if let Some(carriers) = PRELOADED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        carriers.release(keep);
    }
}

/// The preloaded bytes of the carrier at `path`, for readers that need random access; `None`
/// on native builds, which read carriers from disk.
#[must_use]
pub fn preloaded(path: &Path) -> Option<Arc<[u8]>> {
    with_preloaded(|carriers| carriers.files.get(path).cloned()).flatten()
}

/// Where a browser downloads the carrier at `path` that is not in memory yet; `None` for one
/// already in memory and on native builds.
#[must_use]
pub fn remote(path: &Path) -> Option<Arc<str>> {
    with_preloaded(|carriers| {
        (!carriers.files.contains_key(path))
            .then(|| carriers.remote.get(path).cloned())
            .flatten()
    })
    .flatten()
}

/// Installs the downloaded bytes of the [`remote`] carrier at `path`, which then opens like a
/// preloaded one. Native builds, which preload nothing, ignore it.
pub fn add_downloaded(path: &Path, bytes: impl Into<Arc<[u8]>>) {
    if let Some(carriers) = PRELOADED
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        carriers.insert(path, bytes);
    }
}

/// The carrier at `path` that a browser reads by byte range; `None` on native builds.
#[must_use]
pub fn ranged(path: &Path) -> Option<RangedCarrier> {
    with_preloaded(|carriers| carriers.ranged.get(path).cloned()).flatten()
}

/// An open carrier and its length, which loaders bound before reading.
pub struct CarrierFile {
    len: u64,
    source: Source,
}

enum Source {
    File(File),
    Memory(Cursor<Arc<[u8]>>),
}

impl CarrierFile {
    /// The carrier's length in bytes when it was opened.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Whether the carrier holds no bytes.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The carrier's metadata, shaped like a file's so loaders keep their size checks.
    pub fn metadata(&self) -> io::Result<CarrierMetadata> {
        Ok(CarrierMetadata { len: self.len })
    }
}

/// What a loader checks about an open carrier: it is a regular file of `len` bytes.
#[derive(Clone, Copy, Debug)]
pub struct CarrierMetadata {
    len: u64,
}

impl CarrierMetadata {
    /// The carrier's length in bytes.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Carriers are always regular files.
    #[must_use]
    pub const fn is_file(&self) -> bool {
        true
    }
}

impl Read for CarrierFile {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match &mut self.source {
            Source::File(file) => file.read(buffer),
            Source::Memory(cursor) => cursor.read(buffer),
        }
    }
}

/// Opens the carrier at `path`: a preloaded one when a browser installed them, else the file.
pub fn open(path: &Path) -> io::Result<CarrierFile> {
    if let Some(preloaded) = with_preloaded(|carriers| carriers.open(path)) {
        return preloaded;
    }
    let file = File::open(path)?;
    Ok(CarrierFile {
        len: file.metadata()?.len(),
        source: Source::File(file),
    })
}

/// Reads the carrier at `path`, at most `limit + 1` bytes, so a decoder bounded by `limit` sees
/// that an oversized carrier is too long without reading all of it.
pub fn read_bounded(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = open(path)?;
    let mut bytes =
        Vec::with_capacity(usize::try_from(file.len()).map_or(0, |len| len.min(limit + 1)));
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Reads the whole carrier at `path`.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    let mut file = open(path)?;
    let mut bytes = Vec::with_capacity(usize::try_from(file.len()).unwrap_or(0));
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_carriers_read_back_and_report_missing_paths_as_not_found() {
        let mut carriers = MemoryCarriers::default();
        carriers.insert("compiled/world.mcbea", vec![1, 2, 3]);
        let mut file = carriers.open(Path::new("compiled/world.mcbea")).unwrap();
        assert_eq!(file.len(), 3);
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, [1, 2, 3]);
        let missing = carriers.open(Path::new("compiled/absent.mcbea"));
        assert_eq!(
            missing.err().map(|error| error.kind()),
            Some(io::ErrorKind::NotFound)
        );
    }

    #[test]
    fn released_carriers_are_freed_and_kept_ones_still_open() {
        let mut carriers = MemoryCarriers::default();
        let world: Arc<[u8]> = vec![1, 2, 3].into();
        carriers.insert("compiled/world.mcbea", Arc::clone(&world));
        carriers.insert("compiled/lang/de_DE.mcbelang", vec![4]);
        carriers.insert_ranged("compiled/sounds.mcbesnd", vec![5], "sounds");
        carriers.release(|path| path.starts_with("compiled/lang"));
        assert_eq!(Arc::strong_count(&world), 1);
        let released = carriers.open(Path::new("compiled/world.mcbea"));
        assert_eq!(
            released.err().map(|error| error.kind()),
            Some(io::ErrorKind::NotFound)
        );
        assert_eq!(
            carriers
                .open(Path::new("compiled/lang/de_DE.mcbelang"))
                .unwrap()
                .len(),
            1
        );
        assert!(
            carriers
                .ranged
                .contains_key(Path::new("compiled/sounds.mcbesnd"))
        );
    }

    #[test]
    fn files_open_with_their_length() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("carrier.bin");
        std::fs::write(&path, [9; 5]).unwrap();
        assert_eq!(open(&path).unwrap().len(), 5);
        assert_eq!(read(&path).unwrap(), [9; 5]);
    }
}
