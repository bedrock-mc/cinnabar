//! Bounded, read-only admission for server resource-pack archives received during login.
//!
//! Each archive is admitted independently: a bad pack is dropped with a counted
//! reason and the rest of the stack still applies. Encrypted packs are decrypted
//! in memory per read; plaintext is never written anywhere.

use std::sync::Arc;

#[cfg(feature = "handoff")]
use protocol::ResourcePackHandoff;
use thiserror::Error;

mod crypto;
mod dependencies;
mod import;
mod jsonc;
mod library;
mod manifest;
mod merge;
mod pack;
mod parser;
mod view;

pub use dependencies::{PackDependencies, PackDependency};
pub use import::{PACK_IMPORT_EXTENSIONS, is_pack_import_path};
pub use jsonc::normalize_jsonc;
pub use library::{
    ActivePack, GlobalPackLibrary, ImportReport, InstalledPack, LibraryError, Subpack,
};
pub use merge::{MAX_MERGED_ENTRIES, MAX_WINNING_BYTES, MAX_WINNING_FILES};
pub use pack::{PackRejection, ValidatedPack, ValidatedPackStack};
pub use parser::validate_archive_bytes;
pub use view::LayeredPackView;

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
pub const MAX_SUBPACKS: usize = 64;

/// A stable, attacker-data-free reason why the whole selected stack was rejected.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum AdmissionError {
    #[error("resource-pack stack exceeds its pack limit")]
    TooManyPacks,
    #[error("resource-pack identity repeats an earlier stack entry")]
    DuplicatePack,
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
    #[error("resource-pack ZIP compression method is unsupported")]
    UnsupportedCompression,
    #[error("resource-pack file exceeds its uncompressed-size limit")]
    FileTooLarge,
    #[error("resource-pack declared size exceeds its limit")]
    DeclaredSizeTooLarge,
    #[error("resource-pack stack declared size exceeds its limit")]
    StackDeclaredSizeTooLarge,
    #[error("resource-pack content key is invalid")]
    InvalidContentKey,
    #[error("encrypted resource pack has no contents index")]
    MissingContentsIndex,
    #[error("encrypted resource-pack contents index is malformed")]
    MalformedContentsIndex,
    #[error("resource-pack manifest is missing")]
    MissingManifest,
    #[error("resource-pack manifest exceeds its size limit")]
    ManifestTooLarge,
    #[error("resource-pack manifest JSONC is malformed")]
    MalformedManifest,
    #[error("resource-pack manifest format is unsupported")]
    UnsupportedManifestFormat,
    #[error("resource-pack manifest identity does not match the selected pack")]
    ManifestIdentityMismatch,
    #[error("resource-pack manifest version is invalid")]
    InvalidVersion,
    #[error("resource-pack manifest declares no resources module")]
    InvalidModules,
    #[error("resource-pack selected subpack is invalid")]
    InvalidSubpack,
    #[error("resource-pack file data is malformed or inconsistent")]
    InvalidFileData,
}

/// Admission result carried with StartGame. Dropped packs never fail the session.
#[derive(Clone, Debug)]
pub enum PackAdmission {
    None,
    Validated(Arc<ValidatedPackStack>),
}

/// Admits a one-shot handoff pack by pack, preserving stack order.
#[cfg(feature = "handoff")]
#[must_use]
pub fn validate_handoff(handoff: ResourcePackHandoff) -> Arc<ValidatedPackStack> {
    Arc::new(parser::validate_stack(handoff.into_archives()))
}

#[cfg(all(test, feature = "handoff"))]
mod handoff_tests;
