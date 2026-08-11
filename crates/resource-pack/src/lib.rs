//! Bounded, read-only admission for resource-pack archives received during login.
//!
//! Admission indexes each archive and validates its manifest atomically. It does
//! not extract archives, merge pack namespaces, or apply assets.

use std::{
    collections::HashMap,
    io::{Cursor, Read},
    sync::Arc,
};

#[cfg(feature = "handoff")]
use protocol::ResourcePackHandoff;
use thiserror::Error;
use uuid::Uuid;
use zip::ZipArchive;

mod parser;
pub use parser::validate_archive_bytes;

pub const MAX_PACKS: usize = 32;
pub const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_STACK_ARCHIVE_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;
pub const MAX_PATH_BYTES: usize = 512;
pub const MAX_ENTRIES_PER_PACK: usize = 32_768;
pub const MAX_ENTRIES_PER_STACK: usize = 65_536;
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_DECLARED_BYTES_PER_PACK: u64 = 512 * 1024 * 1024;
pub const MAX_DECLARED_BYTES_PER_STACK: u64 = 1024 * 1024 * 1024;
pub const MAX_MODULES: usize = 64;
pub const MAX_DEPENDENCIES: usize = 32;
pub const MAX_SUBPACKS: usize = 64;
pub const MAX_MANIFEST_STRING_BYTES: usize = 1024;

/// A stable, attacker-data-free reason why the whole selected stack was rejected.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum AdmissionError {
    #[error("resource-pack content encryption is unsupported")]
    UnsupportedContentEncryption,
    #[error("resource-pack stack exceeds its pack limit")]
    TooManyPacks,
    #[error("resource-pack archive exceeds its compressed-size limit")]
    ArchiveTooLarge,
    #[error("resource-pack stack exceeds its compressed-size limit")]
    StackArchiveTooLarge,
    #[error("resource-pack ZIP footer is invalid or unsupported")]
    InvalidZipFooter,
    #[error("resource-pack ZIP64 is unsupported")]
    UnsupportedZip64,
    #[error("resource-pack ZIP structure is malformed")]
    MalformedZip,
    #[error("resource-pack ZIP entry count exceeds its limit")]
    TooManyEntries,
    #[error("resource-pack stack entry count exceeds its limit")]
    TooManyStackEntries,
    #[error("resource-pack ZIP encryption is unsupported")]
    UnsupportedZipEncryption,
    #[error("resource-pack ZIP compression method is unsupported")]
    UnsupportedCompression,
    #[error("resource-pack ZIP contains a non-file entry")]
    NonFileEntry,
    #[error("resource-pack ZIP contains an unsafe path")]
    UnsafePath,
    #[error("resource-pack ZIP contains a duplicate or case-colliding path")]
    DuplicatePath,
    #[error("resource-pack file exceeds its uncompressed-size limit")]
    FileTooLarge,
    #[error("resource-pack declared size exceeds its limit")]
    DeclaredSizeTooLarge,
    #[error("resource-pack stack declared size exceeds its limit")]
    StackDeclaredSizeTooLarge,
    #[error("resource-pack manifest is missing")]
    MissingManifest,
    #[error("resource-pack manifest exceeds its size limit")]
    ManifestTooLarge,
    #[error("resource-pack manifest JSONC is malformed")]
    MalformedManifest,
    #[error("resource-pack manifest format is unsupported")]
    UnsupportedManifestFormat,
    #[error("resource-pack manifest text exceeds its limit")]
    ManifestStringTooLong,
    #[error("resource-pack manifest identity does not match the selected pack")]
    ManifestIdentityMismatch,
    #[error("resource-pack manifest version is not canonical")]
    InvalidVersion,
    #[error("resource-pack manifest module declaration is invalid or unsupported")]
    InvalidModules,
    #[error("resource-pack manifest dependency declaration is invalid")]
    InvalidDependencies,
    #[error("resource-pack dependency graph contains a cycle")]
    DependencyCycle,
    #[error("resource-pack selected subpack is invalid")]
    InvalidSubpack,
    #[error("resource-pack file data is malformed or inconsistent")]
    InvalidFileData,
}

/// Admission result carried with StartGame. Optional rejection is non-fatal.
#[derive(Clone, Debug)]
pub enum PackAdmission {
    None,
    Validated(Arc<ValidatedPackStack>),
    Rejected(AdmissionError),
}

impl PackAdmission {
    #[must_use]
    pub const fn is_rejected(&self) -> bool {
        matches!(self, Self::Rejected(_))
    }
}

#[derive(Clone, Debug)]
struct EntryIndex {
    archive_index: usize,
    uncompressed_size: u64,
}

/// One admitted archive. Bytes and its central-directory index are immutable.
pub struct ValidatedPack {
    pack_id: Uuid,
    version: Box<str>,
    sub_pack_name: Box<str>,
    archive: Arc<[u8]>,
    files: HashMap<Box<str>, EntryIndex>,
    file_order: Box<[Box<str>]>,
    dependencies: Box<[(Uuid, parser::Version)]>,
    physical_entry_count: usize,
}

impl std::fmt::Debug for ValidatedPack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValidatedPack")
            .field("archive_bytes", &self.archive.len())
            .field("entry_count", &self.physical_entry_count)
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

    /// Reads one file from this pack only, with a hard uncompressed cap.
    pub fn read_file(&self, path: &str) -> Result<Option<Box<[u8]>>, AdmissionError> {
        self.read_file_with_limit(path, MAX_FILE_BYTES)
    }

    fn read_file_with_limit(
        &self,
        path: &str,
        limit: u64,
    ) -> Result<Option<Box<[u8]>>, AdmissionError> {
        let Some(entry) = self.files.get(path) else {
            return Ok(None);
        };
        if entry.uncompressed_size > limit {
            return Err(AdmissionError::FileTooLarge);
        }
        let mut zip = ZipArchive::new(Cursor::new(Arc::clone(&self.archive)))
            .map_err(|_| AdmissionError::MalformedZip)?;
        let mut file = zip
            .by_index(entry.archive_index)
            .map_err(|_| AdmissionError::InvalidFileData)?;
        if file.size() != entry.uncompressed_size {
            return Err(AdmissionError::InvalidFileData);
        }
        let capacity =
            usize::try_from(entry.uncompressed_size).map_err(|_| AdmissionError::FileTooLarge)?;
        let mut bytes = Vec::with_capacity(capacity);
        let read_limit = entry
            .uncompressed_size
            .saturating_add(1)
            .min(limit.saturating_add(1));
        file.by_ref()
            .take(read_limit)
            .read_to_end(&mut bytes)
            .map_err(|_| AdmissionError::InvalidFileData)?;
        if bytes.len() != capacity || bytes.len() as u64 > limit {
            return Err(AdmissionError::InvalidFileData);
        }
        Ok(Some(bytes.into_boxed_slice()))
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

/// Exact server-selected pack order. No merged lookup is intentionally exposed.
pub struct ValidatedPackStack {
    packs: Box<[ValidatedPack]>,
}

impl std::fmt::Debug for ValidatedPackStack {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValidatedPackStack")
            .field("pack_count", &self.packs.len())
            .finish()
    }
}

impl ValidatedPackStack {
    #[must_use]
    pub fn packs(&self) -> &[ValidatedPack] {
        &self.packs
    }
}

/// Atomically validates a one-shot handoff. No partial stack escapes on error.
#[cfg(feature = "handoff")]
pub fn validate_handoff(
    handoff: ResourcePackHandoff,
) -> Result<Arc<ValidatedPackStack>, AdmissionError> {
    parser::validate_archives(handoff.into_archives()).map(Arc::new)
}

#[cfg(all(test, feature = "handoff"))]
mod handoff_tests {
    use std::io::{Cursor, Write};

    use protocol::{ResourcePackArchive, ResourcePackHandoff};
    use uuid::Uuid;
    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::{AdmissionError, validate_handoff};

    const PACK_ID: Uuid = Uuid::from_u128(0x11111111_2222_3333_4444_555555555555);

    fn archive(bytes: Vec<u8>) -> ResourcePackArchive {
        ResourcePackArchive::unencrypted(PACK_ID, "1.2.3".into(), String::new(), bytes)
    }

    fn valid_zip() -> Vec<u8> {
        let manifest = format!(
            r#"{{"format_version":2,"header":{{"name":"test","description":"test","uuid":"{PACK_ID}","version":[1,2,3]}},"modules":[{{"type":"resources","uuid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","version":[1,2,3]}}]}}"#
        );
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(manifest.as_bytes()).unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn empty_production_handoff_is_validated_without_archives() {
        let stack = validate_handoff(ResourcePackHandoff::default()).expect("empty handoff");
        assert!(stack.packs().is_empty());
    }

    #[test]
    fn nonempty_production_handoff_preserves_selected_metadata() {
        let handoff = ResourcePackHandoff::from_archives(vec![archive(valid_zip())]);
        let stack = validate_handoff(handoff).expect("valid handoff");
        assert_eq!(stack.packs().len(), 1);
        assert_eq!(stack.packs()[0].pack_id(), PACK_ID);
        assert_eq!(stack.packs()[0].version(), "1.2.3");
        assert_eq!(stack.packs()[0].sub_pack_name(), "");
    }

    #[test]
    fn nonempty_production_handoff_rejects_malformed_archive_atomically() {
        let handoff = ResourcePackHandoff::from_archives(vec![archive(vec![0; 32])]);
        assert!(matches!(
            validate_handoff(handoff),
            Err(AdmissionError::InvalidZipFooter)
        ));
    }
}
