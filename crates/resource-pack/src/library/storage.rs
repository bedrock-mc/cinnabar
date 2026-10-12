//! Where installed packs live, and retiring immutable archives only after their replacement has
//! been acknowledged.
use super::{CATALOG_FILE, Catalog, LibraryError, encode_catalog};
use crate::AdmissionError;
use std::{
    fs,
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
};

/// The library's catalog, archives and icons, each under a flat file name: a directory on
/// desktops, the browser's own storage in a browser.
pub trait PackStorage: Send {
    /// The bytes of `name`; more than `limit` bytes is [`AdmissionError::ArchiveTooLarge`], and an
    /// absent file is an I/O error of kind `NotFound`.
    fn read(&self, name: &str, limit: usize) -> Result<Vec<u8>, LibraryError>;
    /// Whether `name` is stored.
    fn contains(&self, name: &str) -> bool;
    /// Replaces `name` so readers see either its old or its new bytes.
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), LibraryError>;
    /// Removes `name`; one already absent is an I/O error of kind `NotFound`.
    fn remove(&mut self, name: &str) -> Result<(), LibraryError>;
    /// The path the menus open `name` at.
    fn path(&self, name: &str) -> PathBuf;
}

/// Packs in a directory, created on the first write.
pub struct DirectoryStorage {
    root: PathBuf,
}

impl DirectoryStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

/// Reads the file at `path`, capped before allocating, including a file that grows during the
/// read.
pub(super) fn read_file(path: &Path, limit: usize) -> Result<Vec<u8>, LibraryError> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(AdmissionError::ArchiveTooLarge.into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(AdmissionError::ArchiveTooLarge.into());
    }
    Ok(bytes)
}

impl PackStorage for DirectoryStorage {
    fn read(&self, name: &str, limit: usize) -> Result<Vec<u8>, LibraryError> {
        read_file(&self.root.join(name), limit)
    }

    fn contains(&self, name: &str) -> bool {
        self.root.join(name).is_file()
    }

    /// Uses a sibling temporary file so readers see either complete version.
    fn write(&mut self, name: &str, bytes: &[u8]) -> Result<(), LibraryError> {
        use std::io::Write;
        let path = self.root.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    }

    fn remove(&mut self, name: &str) -> Result<(), LibraryError> {
        Ok(fs::remove_file(self.root.join(name))?)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

/// Whether `error` reports a missing file.
pub(super) fn not_found(error: &LibraryError) -> bool {
    matches!(error, LibraryError::Io(error) if error.kind() == ErrorKind::NotFound)
}

/// Removes obsolete revisions after the newest preview has been acknowledged.
pub(super) fn prune(
    storage: &mut dyn PackStorage,
    catalog: &mut Catalog,
) -> Result<(), LibraryError> {
    let mut candidate = catalog.clone();
    let mut failure = None;
    candidate.retained.retain(|pack| {
        if candidate
            .active
            .iter()
            .any(|active| active.id == pack.id && active.revision == pack.revision)
        {
            return true;
        }
        match storage.remove(&pack.filename()) {
            Ok(()) => false,
            Err(error) if not_found(&error) => false,
            Err(error) => {
                failure.get_or_insert(error);
                true
            }
        }
    });
    storage.write(CATALOG_FILE, &encode_catalog(&candidate)?)?;
    *catalog = candidate;
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
