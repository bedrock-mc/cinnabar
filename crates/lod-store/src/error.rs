//! Region file errors and the recoveries a reopened file reports.

use std::io;

use lod::{codec::CodecError, geometry::SectionKey};

use crate::{frame::FrameError, region_id::RegionId, region_index::IndexError};

/// Why a region file's fixed header was rejected; the file cannot be used and is replaced
/// by a writer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HeaderError {
    #[error("region file has {0} bytes, less than its fixed header")]
    Truncated(u64),
    #[error("region file starts with {0:02x?}, not the region magic")]
    Magic([u8; 4]),
    #[error("region format version {0} is not supported")]
    Version(u16),
    #[error("region header checksum {stored:#010x} does not match {computed:#010x}")]
    Checksum { stored: u32, computed: u32 },
    #[error("region header names level {level} at {x},{z}, not {expected:?}")]
    Identity {
        expected: RegionId,
        level: u8,
        x: i32,
        z: i32,
    },
}

/// Why a region file operation failed.
#[derive(Debug, thiserror::Error)]
pub enum RegionError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("region file header is invalid: {0}")]
    Header(#[from] HeaderError),
    /// The stored record is unreadable; the caller rebuilds it from its chunks.
    #[error("stored record of {key:?} at offset {offset} is corrupt: {error}")]
    Corrupt {
        key: SectionKey,
        offset: u64,
        error: FrameError,
    },
    #[error("record to store is invalid: {0}")]
    Record(#[from] CodecError),
    #[error("{key:?} does not belong to region {region:?}")]
    KeyOutsideRegion { key: SectionKey, region: RegionId },
    #[error("chunk {x},{z} does not belong to region {region:?}")]
    ChunkOutsideRegion { x: i32, z: i32, region: RegionId },
    #[error("region {0:?} is not level 0 and holds no chunk hashes")]
    NotLevelZero(RegionId),
    #[error("region file is open read-only")]
    ReadOnly,
}

/// Why a reopened file's committed index could not be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IndexFault {
    #[error("no commit slot is valid")]
    NoCommit,
    #[error("index frame at {offset} is unreadable: {error}")]
    Frame { offset: u64, error: FrameError },
    #[error("index frame at {offset} is malformed: {error}")]
    Index { offset: u64, error: IndexError },
    #[error("index chain at {offset} is longer than its bound or does not move backwards")]
    Chain { offset: u64 },
    #[error("index entry for slot {slot} points outside the file")]
    EntryOutOfBounds { slot: u32 },
}

/// How a reopened file rebuilt its index by scanning its frames.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recovery {
    /// Why the committed index was not used.
    pub fault: IndexFault,
    /// Records the scan found.
    pub records: usize,
    /// Bytes skipped as unreadable or invalid frames.
    pub skipped_bytes: u64,
}

/// Why a store operation failed.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Region(#[from] RegionError),
    #[error(transparent)]
    Table(#[from] TableErrorAt),
    /// The material registry is unreadable; a writer resets the store instead.
    #[error("material registry is unreadable: {0}")]
    Registry(#[from] lod::registry::RegistryError),
    #[error("store is open read-only")]
    ReadOnly,
    #[error("no session is open in dimension {0:?}")]
    NoSession(crate::identity::Dimension),
    #[error("every epoch id of this store is used")]
    EpochIdsExhausted,
}

/// A table file error with the path of the file.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {error}", path.display())]
pub struct TableErrorAt {
    pub path: std::path::PathBuf,
    pub error: crate::table::TableError,
}
