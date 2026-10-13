//! One destination world's store: its lock, material registry, epoch catalog, access
//! table, and the region and signature files of each epoch.
//!
//! Layout below the store directory:
//!
//! - [`crate::lock::LOCK_FILE`], held by the one writer; a store opened without it is read-only.
//! - [`MATERIALS_FILE`], the [`lod::registry`] table every record's material ids refer to.
//!   It is saved and synced before any region frame reaches disk, whether by a flush, an
//!   evicted handle's commit, a merge or a full write buffer, so a power loss never
//!   leaves records whose materials are missing; an unreadable registry resets the whole
//!   store.
//! - [`crate::epochs::CATALOG_FILE`], [`crate::access::ACCESS_FILE`] and
//!   [`crate::access::OPENED_FILE`], rebuilt or reset when unreadable.
//! - `dim<id>/e<epoch>/`: the epoch's region files and surface signature files.
//!
//! Everything else is a cache with the durability of [`crate::region_file`]: commits are
//! not fsynced, and data lost to a crash is sampled again.

mod data;
mod events;
mod evict;
mod open;
mod persist;
mod registry;
mod session;
#[cfg(test)]
mod tests;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use foldhash::fast::FixedState;
use lod::registry::MaterialRegistry;

pub(crate) use self::persist::table_error;
pub use self::{
    events::{ResetReason, StoreEvent},
    evict::Focus,
    session::EpochStatus,
};
use crate::{
    access::AccessTable,
    epochs::{EpochCatalog, EpochRef},
    identity::Dimension,
    lock::StoreLock,
    minute::Minute,
    region_file::Access,
    region_set::{COMPACTION_BUDGET_BYTES, DEFAULT_OPEN_REGIONS, RegionSet},
    signatures::{DEFAULT_OPEN_SIGNATURE_FILES, SignatureCache},
};

/// Name of the material registry file inside a store directory.
pub const MATERIALS_FILE: &str = "materials.clmr";
/// Largest registry file read: every material with a long custom key.
pub const MAX_MATERIALS_BYTES: usize = 64 * 1024 * 1024;

/// The store's material registry, shared with samplers that assign ids while the store
/// persists it.
pub type SharedRegistry = Arc<RwLock<MaterialRegistry>>;

/// Bounds on what an open store keeps in memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreOptions {
    /// Open region file handles across every epoch of the store.
    pub open_regions: NonZeroUsize,
    /// Signature files in memory per epoch.
    pub open_signature_files: NonZeroUsize,
    /// Bytes of live data one flush may rewrite by compaction per epoch, beyond the
    /// first file; see [`RegionSet::commit_all`].
    pub compaction_budget_bytes: u64,
}

impl Default for StoreOptions {
    fn default() -> Self {
        Self {
            open_regions: DEFAULT_OPEN_REGIONS,
            open_signature_files: DEFAULT_OPEN_SIGNATURE_FILES,
            compaction_budget_bytes: COMPACTION_BUDGET_BYTES,
        }
    }
}

/// The files of one open epoch.
#[derive(Debug)]
struct EpochFiles {
    regions: RegionSet,
    signatures: SignatureCache,
}

/// One open store. Dropping it discards uncommitted changes; owners call
/// [`Self::flush`] periodically and [`Self::close`] at the end. Every epoch a session
/// reads, writes or votes against stays open until that session ends, so owners end the
/// session of a dimension they leave for good; open files stay within
/// [`StoreOptions::open_regions`] across all of them.
#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
    _lock: Option<StoreLock>,
    access: Access,
    options: StoreOptions,
    registry: Arc<registry::RegistryFile>,
    /// This open's minute, or a later one recorded before.
    opened: Minute,
    catalog: EpochCatalog,
    catalog_dirty: bool,
    table: AccessTable,
    table_dirty: bool,
    epochs: HashMap<EpochRef, EpochFiles, FixedState>,
    /// Epochs a read-only store created for its own sessions; they live only in memory.
    detached: HashSet<EpochRef, FixedState>,
    sessions: BTreeMap<Dimension, session::Session>,
    events: Vec<StoreEvent>,
}

impl Store {
    /// The store directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Whether this store holds the lock and may write.
    #[must_use]
    pub fn access(&self) -> Access {
        self.access
    }

    /// The material registry, shared with samplers.
    #[must_use]
    pub fn registry(&self) -> SharedRegistry {
        Arc::clone(self.registry.shared())
    }

    /// When the store was last opened: this open, unless an earlier one recorded a later
    /// minute.
    #[must_use]
    pub fn opened(&self) -> Minute {
        self.opened
    }

    /// The epoch catalog.
    #[must_use]
    pub fn catalog(&self) -> &EpochCatalog {
        &self.catalog
    }

    /// The access table.
    #[must_use]
    pub fn access_table(&self) -> &AccessTable {
        &self.table
    }
}
