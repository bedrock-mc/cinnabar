//! What a store reports to its owner.

use std::path::PathBuf;

use lod::epoch::RegionPos;

use super::EpochStatus;
use crate::{
    access::RegionRef, epochs::EpochRef, error::TableErrorAt, identity::Dimension,
    region_set::RegionEvent, signatures::SignatureEvent,
};

/// Something a store did on its own that its owner should count and log.
#[derive(Debug)]
pub enum StoreEvent {
    /// A region file event of one epoch.
    Region { epoch: EpochRef, event: RegionEvent },
    /// A signature file event of one epoch.
    Signature {
        epoch: EpochRef,
        event: SignatureEvent,
    },
    /// The registry was unreadable, so every epoch was deleted.
    Reset(ResetReason),
    /// The catalog was unreadable and was rebuilt from the epoch directories.
    CatalogRebuilt(TableErrorAt),
    /// The access table was unreadable and was reset.
    AccessReset(TableErrorAt),
    /// A session's epoch changed state.
    Epoch {
        dimension: Dimension,
        status: EpochStatus,
    },
    /// A signature region of an undecided session voted against the stored epoch it was
    /// compared with; sections overlapping it no longer read through to that epoch.
    ReadThroughHidden {
        dimension: Dimension,
        region: RegionPos,
        epoch: EpochRef,
    },
    /// An undecided candidate was discarded with its data.
    CandidateDropped(EpochRef),
    /// An established epoch was deleted to keep the per-store bound.
    EpochEvicted(EpochRef),
    /// A region file was deleted to keep a quota.
    RegionEvicted { region: RegionRef, bytes: u64 },
    /// Writing or deleting a store file failed.
    Io {
        path: PathBuf,
        error: std::io::Error,
    },
}

/// Why a store was reset.
#[derive(Debug)]
pub enum ResetReason {
    /// The registry file failed its envelope or size checks.
    File(TableErrorAt),
    /// The registry file decoded to an invalid table.
    Registry(lod::registry::RegistryError),
}
