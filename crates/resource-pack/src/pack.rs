//! Admitted packs and the server-ordered stack they form.

use std::{
    collections::HashMap,
    io::{Cursor, Read},
    sync::{Arc, OnceLock},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::{CompressionMethod, ZipArchive};

use crate::{AdmissionError, MAX_FILE_BYTES, crypto::ContentKey, normalize_jsonc};

/// A parsed archive handle. Cloning shares the central directory (`Arc`) and
/// only copies the cursor, so a per-read clone avoids re-parsing.
pub(crate) type PackZip = ZipArchive<Cursor<Arc<[u8]>>>;

#[derive(Clone, Debug)]
pub(crate) struct EntryIndex {
    pub(crate) archive_index: usize,
    pub(crate) uncompressed_size: u64,
    pub(crate) key: Option<usize>,
}

/// One admitted archive. Bytes stay compressed (and encrypted, if they were);
/// files are inflated and decrypted per read, so plaintext never persists.
#[derive(Clone)]
pub struct ValidatedPack {
    pub(crate) pack_id: Uuid,
    pub(crate) version: Box<str>,
    pub(crate) sub_pack_name: Box<str>,
    pub(crate) archive_bytes: usize,
    pub(crate) declared_bytes: u64,
    pub(crate) zip: PackZip,
    pub(crate) files: HashMap<Box<str>, EntryIndex>,
    pub(crate) folded: HashMap<Box<str>, Box<str>>,
    pub(crate) file_order: Box<[Box<str>]>,
    pub(crate) keys: Arc<[ContentKey]>,
    pub(crate) physical_entry_count: usize,
    pub(crate) skipped_entries: usize,
    pub(crate) compilation_identity: Arc<OnceLock<[u8; 32]>>,
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
    /// Identifies the immutable archive, selected files and their decryption inputs.
    /// Clones share the digest; cache comparisons never expose the content keys.
    #[must_use]
    pub fn compilation_identity(&self) -> [u8; 32] {
        *self.compilation_identity.get_or_init(|| {
            let mut hash = Sha256::new();
            hash.update(self.pack_id.as_bytes());
            for bytes in [self.version.as_bytes(), self.sub_pack_name.as_bytes()] {
                hash.update((bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
            }
            let archive = self.archive_bytes();
            hash.update((archive.len() as u64).to_le_bytes());
            hash.update(&*archive);
            for path in &self.file_order {
                let entry = &self.files[path];
                hash.update((path.len() as u64).to_le_bytes());
                hash.update(path.as_bytes());
                hash.update((entry.archive_index as u64).to_le_bytes());
                match entry.key {
                    Some(key) => {
                        hash.update([1]);
                        hash.update(self.keys[key].identity());
                    }
                    None => hash.update([0]),
                }
            }
            hash.finalize().into()
        })
    }

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
    /// unsupported codecs, oversized files).
    #[must_use]
    pub fn skipped_entries(&self) -> usize {
        self.skipped_entries
    }

    /// The admitted archive bytes, still compressed and (if applicable) encrypted.
    #[must_use]
    pub fn archive_bytes(&self) -> Arc<[u8]> {
        self.zip.clone().into_inner().into_inner()
    }

    /// Whether `path` exists, matching case-insensitively when no exact match exists.
    #[must_use]
    pub fn contains(&self, path: &str) -> bool {
        self.entry(path).is_some()
    }

    /// Reads one decrypted file from this pack only, with a hard uncompressed cap.
    pub fn read_file(&self, path: &str) -> Result<Option<Box<[u8]>>, AdmissionError> {
        self.read_file_with_limit(path, MAX_FILE_BYTES)
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
        let raw = read_entry(&self.zip, entry, limit)?;
        let Some(key) = entry.key.map(|index| &self.keys[index]) else {
            return Ok(Some(raw));
        };
        let mut decrypted = raw.to_vec();
        key.decrypt(&mut decrypted);
        // Some packs list a key for a file stored as plaintext; for JSON keep
        // whichever side parses.
        let is_json = path
            .rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("json"));
        if is_json && normalize_jsonc(&decrypted).is_none() && normalize_jsonc(&raw).is_some() {
            return Ok(Some(raw));
        }
        Ok(Some(decrypted.into_boxed_slice()))
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
    let mut bytes = Vec::with_capacity(capacity);
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

/// Admitted packs in exact server stack order; the last has the highest precedence.
pub struct ValidatedPackStack {
    pub(crate) packs: Box<[ValidatedPack]>,
    pub(crate) rejections: Box<[PackRejection]>,
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
        use crate::{
            MAX_DECLARED_BYTES_PER_STACK, MAX_ENTRIES_PER_STACK, MAX_PACKS, MAX_STACK_ARCHIVE_BYTES,
        };
        let layers = lower.packs.iter().chain(higher.packs.iter());
        if layers.clone().count() > MAX_PACKS {
            return Err(AdmissionError::TooManyPacks);
        }
        if layers.clone().map(|pack| pack.archive_bytes).sum::<usize>() > MAX_STACK_ARCHIVE_BYTES {
            return Err(AdmissionError::StackArchiveTooLarge);
        }
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
        })
    }
    #[must_use]
    pub fn packs(&self) -> &[ValidatedPack] {
        &self.packs
    }

    #[must_use]
    pub fn rejections(&self) -> &[PackRejection] {
        &self.rejections
    }
}
