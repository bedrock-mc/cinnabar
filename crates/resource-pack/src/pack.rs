//! Admitted packs and the server-ordered stack they form.

use std::{
    collections::HashMap,
    io::{Cursor, Read},
    sync::{Arc, OnceLock},
};

use uuid::Uuid;
use zip::{CompressionMethod, ZipArchive};

use pack_json::validate_jsonc;

use crate::{
    AdmissionError,
    crypto::ContentKey,
    read_cache::{CachedEntry, ReadCache},
};

/// A parsed archive handle. Cloning shares the central directory (`Arc`) and
/// only copies the cursor, so a per-read clone avoids re-parsing.
pub(crate) type PackZip = ZipArchive<Cursor<ArchiveBytes>>;

/// Shares the archive's original allocation while exposing a slice to the ZIP cursor.
#[derive(Clone, Debug)]
pub(crate) struct ArchiveBytes(pub(crate) Arc<Vec<u8>>);

impl AsRef<[u8]> for ArchiveBytes {
    /// Borrows the archive payload without copying or changing its allocation.
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EntryIndex {
    pub(crate) archive_index: usize,
    pub(crate) uncompressed_size: u64,
    pub(crate) key: Option<usize>,
    pub(crate) cached: Arc<CachedEntry>,
}

/// One admitted archive, sharing bounded inflated/decrypted reads and their hashes
/// across subscribers, pack clones and composed stacks.
#[derive(Clone)]
pub struct ValidatedPack {
    pub(crate) pack_id: Uuid,
    pub(crate) version: Box<str>,
    pub(crate) sub_pack_name: Box<str>,
    pub(crate) archive_bytes: usize,
    pub(crate) declared_bytes: u64,
    pub(crate) zip: PackZip,
    pub(crate) content_hash: Arc<crate::fingerprint::ContentHash>,
    pub(crate) reads: Arc<ReadCache>,
    pub(crate) files: HashMap<Box<str>, EntryIndex>,
    pub(crate) folded: HashMap<Box<str>, Box<str>>,
    pub(crate) file_order: Box<[Box<str>]>,
    pub(crate) keys: Arc<[ContentKey]>,
    pub(crate) physical_entry_count: usize,
    pub(crate) skipped_entries: usize,
}

impl std::fmt::Debug for ValidatedPack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValidatedPack")
            .field("archive_bytes", &self.archive_bytes)
            .field("entry_count", &self.physical_entry_count)
            .field("encrypted", &!self.keys.is_empty())
            .finish_non_exhaustive()
    }
}

impl ValidatedPack {
    #[must_use]
    pub const fn pack_id(&self) -> Uuid {
        self.pack_id
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub fn sub_pack_name(&self) -> &str {
        &self.sub_pack_name
    }

    #[must_use]
    pub fn entry_count(&self) -> usize {
        self.physical_entry_count
    }

    /// Entries dropped at admission (directories, unsafe or duplicate paths,
    /// unsupported codecs).
    #[must_use]
    pub fn skipped_entries(&self) -> usize {
        self.skipped_entries
    }

    /// The admitted archive bytes, still compressed and (if applicable) encrypted.
    #[must_use]
    pub fn archive_bytes(&self) -> Arc<Vec<u8>> {
        self.zip.clone().into_inner().into_inner().0
    }

    /// Whether `path` exists, matching case-insensitively when no exact match exists.
    #[must_use]
    pub fn contains(&self, path: &str) -> bool {
        self.entry(path).is_some()
    }

    /// The admitted uncompressed entry size, without inflating or decrypting its bytes.
    #[must_use]
    pub fn file_size(&self, path: &str) -> Option<u64> {
        self.entry(path).map(|entry| entry.uncompressed_size)
    }

    /// Reads one complete decrypted file from this pack, checking its declared size.
    pub fn read_file(&self, path: &str) -> Result<Option<Box<[u8]>>, AdmissionError> {
        self.read_file_with_limit(path, u64::MAX)
    }

    /// Compares pack identity, logical index and file keys without reading archive bytes.
    /// Together with the archive digest, this preserves reads, inflation and decryption.
    fn same_index(&self, other: &Self) -> bool {
        /// Resolves an indexed entry's optional content key for semantic pack comparisons.
        fn key<'a>(pack: &'a ValidatedPack, entry: &EntryIndex) -> Option<&'a ContentKey> {
            entry.key.map(|index| &pack.keys[index])
        }
        self.pack_id == other.pack_id
            && self.version == other.version
            && self.sub_pack_name == other.sub_pack_name
            && self.archive_bytes == other.archive_bytes
            && self.declared_bytes == other.declared_bytes
            && self.physical_entry_count == other.physical_entry_count
            && self.skipped_entries == other.skipped_entries
            && self.file_order == other.file_order
            && self.folded == other.folded
            && self.files.len() == other.files.len()
            && self.files.iter().all(|(path, entry)| {
                other.files.get(path).is_some_and(|theirs| {
                    entry.archive_index == theirs.archive_index
                        && entry.uncompressed_size == theirs.uncompressed_size
                        && match (key(self, entry), key(other, theirs)) {
                            (Some(ours), Some(theirs)) => ours.same_key(theirs),
                            (None, None) => true,
                            _ => false,
                        }
                })
            })
    }

    pub(crate) fn entry(&self, path: &str) -> Option<&EntryIndex> {
        self.files.get(path).or_else(|| {
            let exact = self.folded.get(path.to_ascii_lowercase().as_str())?;
            self.files.get(exact)
        })
    }

    /// Reads one layer with a subscriber-specific uncompressed byte limit.
    pub fn read_file_with_limit(
        &self,
        path: &str,
        limit: u64,
    ) -> Result<Option<Box<[u8]>>, AdmissionError> {
        let Some(entry) = self.entry(path) else {
            return Ok(None);
        };
        if entry.uncompressed_size > limit {
            return Err(AdmissionError::FileTooLarge);
        }
        self.reads
            .read(&entry.cached, || self.read_uncached(path, entry, limit))
            .map(|bytes| Some(bytes.as_ref().clone()))
    }

    /// Hashes the same bounded plaintext returned by a file read, retaining the hash
    /// even when the shared byte cache evicts the payload.
    pub fn file_fingerprint_with_limit(
        &self,
        path: &str,
        limit: u64,
    ) -> Result<Option<[u8; 32]>, AdmissionError> {
        let Some(entry) = self.entry(path) else {
            return Ok(None);
        };
        if entry.uncompressed_size > limit {
            return Err(AdmissionError::FileTooLarge);
        }
        self.reads
            .fingerprint(&entry.cached, || self.read_uncached(path, entry, limit))
            .map(Some)
    }

    /// Inflates and decrypts one entry without consulting or modifying its cache.
    fn read_uncached(
        &self,
        path: &str,
        entry: &EntryIndex,
        limit: u64,
    ) -> Result<Box<[u8]>, AdmissionError> {
        let raw = read_entry(&self.zip, entry, limit)?;
        let Some(key) = entry.key.map(|index| &self.keys[index]) else {
            return Ok(raw);
        };
        let mut decrypted = raw.to_vec();
        key.decrypt(&mut decrypted);
        // Some packs list a key for a file stored as plaintext; for JSON keep
        // whichever side parses.
        let is_json = path
            .rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("json"));
        if is_json && validate_jsonc(&decrypted).is_err() && validate_jsonc(&raw).is_ok() {
            return Ok(raw);
        }
        Ok(decrypted.into_boxed_slice())
    }

    /// Lists logical files under `prefix` in deterministic lexical order.
    #[must_use]
    pub fn files_under<'a>(&'a self, prefix: &str) -> Box<[&'a str]> {
        self.file_order
            .iter()
            .map(Box::as_ref)
            .filter(|path| path.starts_with(prefix))
            .collect()
    }
}

/// Reads an indexed member within the caller's allowance and checks its complete output.
pub(crate) fn read_entry(
    zip: &PackZip,
    entry: &EntryIndex,
    limit: u64,
) -> Result<Box<[u8]>, AdmissionError> {
    if entry.uncompressed_size > limit {
        return Err(AdmissionError::FileTooLarge);
    }
    // Clone shares the parsed directory; only the cursor is copied.
    let mut zip = zip.clone();
    let raw = zip
        .by_index_raw(entry.archive_index)
        .map_err(|_| AdmissionError::InvalidFileData)?;
    if !matches!(
        raw.compression(),
        CompressionMethod::Stored | CompressionMethod::Deflated
    ) {
        return Err(AdmissionError::UnsupportedCompression);
    }
    drop(raw);
    let mut file = zip
        .by_index(entry.archive_index)
        .map_err(|_| AdmissionError::InvalidFileData)?;
    if file.size() != entry.uncompressed_size {
        return Err(AdmissionError::InvalidFileData);
    }
    let capacity =
        usize::try_from(entry.uncompressed_size).map_err(|_| AdmissionError::FileTooLarge)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(capacity)
        .map_err(|_| AdmissionError::FileTooLarge)?;
    file.by_ref()
        .take(entry.uncompressed_size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| AdmissionError::InvalidFileData)?;
    if bytes.len() != capacity {
        return Err(AdmissionError::InvalidFileData);
    }
    Ok(bytes.into_boxed_slice())
}

/// Why one stack entry was dropped; it carries no pack identity or content.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PackRejection {
    pub stack_index: usize,
    pub reason: AdmissionError,
}

/// Admitted layers in lowest-to-highest precedence order.
pub struct ValidatedPackStack {
    pub(crate) packs: Box<[ValidatedPack]>,
    pub(crate) rejections: Box<[PackRejection]>,
    pub(crate) fingerprint: OnceLock<crate::StackFingerprint>,
}

impl std::fmt::Debug for ValidatedPackStack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValidatedPackStack")
            .field("pack_count", &self.packs.len())
            .field("rejections", &self.rejections)
            .finish()
    }
}

impl ValidatedPackStack {
    /// Combines optional layers in increasing precedence, sharing archive bytes.
    pub fn compose(lower: &Self, higher: &Self) -> Result<Self, AdmissionError> {
        use crate::{MAX_DECLARED_BYTES_PER_STACK, MAX_ENTRIES_PER_STACK};
        let layers = lower.packs.iter().chain(higher.packs.iter());
        if layers
            .clone()
            .map(ValidatedPack::entry_count)
            .sum::<usize>()
            > MAX_ENTRIES_PER_STACK
        {
            return Err(AdmissionError::TooManyStackEntries);
        }
        if layers.clone().map(|pack| pack.declared_bytes).sum::<u64>()
            > MAX_DECLARED_BYTES_PER_STACK
        {
            return Err(AdmissionError::StackDeclaredSizeTooLarge);
        }
        let offset = lower.packs.len() + lower.rejections.len();
        let rejections = lower
            .rejections
            .iter()
            .copied()
            .chain(higher.rejections.iter().map(|rejection| PackRejection {
                stack_index: offset + rejection.stack_index,
                reason: rejection.reason,
            }))
            .collect();
        Ok(Self {
            packs: layers.cloned().collect(),
            rejections,
            fingerprint: OnceLock::new(),
        })
    }
    /// Admitted layers, lowest precedence first.
    #[must_use]
    pub fn packs(&self) -> &[ValidatedPack] {
        &self.packs
    }

    /// Returns the admission-time archive digests without hashing composed copies again.
    #[must_use]
    pub fn fingerprint(&self) -> &crate::StackFingerprint {
        use rayon::prelude::*;
        self.fingerprint.get_or_init(|| {
            self.packs
                .par_iter()
                .map(|pack| {
                    (
                        pack.pack_id.to_string(),
                        pack.version.to_string(),
                        pack.sub_pack_name.to_string(),
                        pack.content_hash.get(&pack.archive_bytes()),
                    )
                })
                .collect()
        })
    }

    /// Compares archive digests, layered reads and dropped packs without scanning archive bytes.
    #[must_use]
    pub fn same_contents(&self, other: &Self) -> bool {
        self.rejections == other.rejections
            && self.fingerprint() == other.fingerprint()
            && self.packs.len() == other.packs.len()
            && self
                .packs
                .iter()
                .zip(other.packs.iter())
                .all(|(ours, theirs)| ours.same_index(theirs))
    }

    #[must_use]
    pub fn rejections(&self) -> &[PackRejection] {
        &self.rejections
    }
}

#[cfg(test)]
#[path = "pack_tests.rs"]
mod tests;
