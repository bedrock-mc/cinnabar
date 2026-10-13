//! A store's catalog of world epochs: which epochs each dimension holds, whether each is
//! established or still a session's candidate, and when each was last used.

use std::path::Path;

use crate::{
    identity::Dimension,
    minute::Minute,
    table::{BodyReader, TableError, seal, unseal},
};

/// Leading bytes of the catalog file.
pub const CATALOG_MAGIC: [u8; 4] = *b"CLEP";
/// Version of the catalog layout.
pub const CATALOG_FORMAT_VERSION: u16 = 1;
/// Name of the catalog file inside a store directory.
pub const CATALOG_FILE: &str = "epochs.clep";
/// Established epochs one store keeps; creating one more evicts the least recently used.
pub const MAX_EPOCHS_PER_STORE: usize = 8;
/// Largest catalog file read.
pub const MAX_CATALOG_BYTES: usize = 64 * 1024;

const EPOCH_DIR_PREFIX: &str = "e";
const ENTRY_LEN: usize = 13;
const ESTABLISHED: u8 = 0;
const CANDIDATE: u8 = 1;

/// One epoch's number, unique within its store and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EpochId(pub u32);

impl EpochId {
    /// The epoch's directory name inside its dimension directory.
    #[must_use]
    pub fn dir_name(self) -> String {
        format!("{EPOCH_DIR_PREFIX}{}", self.0)
    }

    /// Parses a name written by [`Self::dir_name`]; `None` for any other name.
    #[must_use]
    pub fn from_dir_name(name: &str) -> Option<Self> {
        let id = Self(name.strip_prefix(EPOCH_DIR_PREFIX)?.parse().ok()?);
        (id.dir_name() == name).then_some(id)
    }
}

/// One epoch of one dimension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EpochRef {
    pub dimension: Dimension,
    pub id: EpochId,
}

impl EpochRef {
    /// The epoch's directory below the store directory `store_dir`.
    #[must_use]
    pub fn dir(self, store_dir: &Path) -> std::path::PathBuf {
        store_dir
            .join(self.dimension.dir_name())
            .join(self.id.dir_name())
    }
}

/// Whether an epoch is a known world or a session still being compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EpochState {
    Established,
    /// A session's own data while its votes are undecided; discarded unless it merges
    /// or splits.
    Candidate,
}

/// One catalog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochEntry {
    pub epoch: EpochRef,
    pub state: EpochState,
    pub last_used: Minute,
}

/// Every epoch of one store.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EpochCatalog {
    next_id: u32,
    entries: Vec<EpochEntry>,
}

impl EpochCatalog {
    /// An empty catalog.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A catalog of the epochs whose directories exist, all established and last used at
    /// `now`; used when the catalog file is lost.
    #[must_use]
    pub fn rebuilt(epochs: impl IntoIterator<Item = EpochRef>, now: Minute) -> Self {
        let mut catalog = Self::new();
        for epoch in epochs {
            if catalog.get(epoch).is_none() {
                catalog.next_id = catalog.next_id.max(epoch.id.0.saturating_add(1));
                catalog.entries.push(EpochEntry {
                    epoch,
                    state: EpochState::Established,
                    last_used: now,
                });
            }
        }
        catalog.entries.sort_by_key(|entry| entry.epoch);
        catalog
    }

    /// Adds a new epoch of `dimension`; `None` once every id is used.
    pub fn create(
        &mut self,
        dimension: Dimension,
        state: EpochState,
        now: Minute,
    ) -> Option<EpochRef> {
        let id = EpochId(self.next_id);
        self.next_id = self.next_id.checked_add(1)?;
        let epoch = EpochRef { dimension, id };
        self.entries.push(EpochEntry {
            epoch,
            state,
            last_used: now,
        });
        Some(epoch)
    }

    /// The entry of `epoch`.
    #[must_use]
    pub fn get(&self, epoch: EpochRef) -> Option<&EpochEntry> {
        self.entries.iter().find(|entry| entry.epoch == epoch)
    }

    /// Marks `epoch` established; false when it is not catalogued.
    pub fn establish(&mut self, epoch: EpochRef) -> bool {
        self.entry_mut(epoch)
            .map(|entry| entry.state = EpochState::Established)
            .is_some()
    }

    /// Records a use of `epoch` at `now`.
    pub fn touch(&mut self, epoch: EpochRef, now: Minute) {
        if let Some(entry) = self.entry_mut(epoch) {
            entry.last_used = entry.last_used.max(now);
        }
    }

    /// Removes `epoch`; false when it was not catalogued.
    pub fn remove(&mut self, epoch: EpochRef) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| entry.epoch != epoch);
        self.entries.len() != before
    }

    /// Every entry, in creation order.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &EpochEntry> {
        self.entries.iter()
    }

    /// Established epochs of `dimension`, in creation order.
    pub fn established(&self, dimension: Dimension) -> impl Iterator<Item = EpochRef> + '_ {
        self.entries
            .iter()
            .filter(move |entry| {
                entry.epoch.dimension == dimension && entry.state == EpochState::Established
            })
            .map(|entry| entry.epoch)
    }

    /// The established epoch to evict while more than [`MAX_EPOCHS_PER_STORE`] exist:
    /// the least recently used one outside `keep`.
    #[must_use]
    pub fn over_limit(&self, keep: &[EpochRef]) -> Option<EpochRef> {
        let established = self
            .entries
            .iter()
            .filter(|entry| entry.state == EpochState::Established);
        if established.clone().count() <= MAX_EPOCHS_PER_STORE {
            return None;
        }
        established
            .filter(|entry| !keep.contains(&entry.epoch))
            .min_by_key(|entry| (entry.last_used, entry.epoch.id))
            .map(|entry| entry.epoch)
    }

    /// The sealed catalog: next id, entry count, then each entry's dimension, id, state
    /// and last-used minute, little-endian.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::with_capacity(8 + self.entries.len() * ENTRY_LEN);
        body.extend_from_slice(&self.next_id.to_le_bytes());
        body.extend_from_slice(&(self.entries.len() as u32).to_le_bytes());
        for entry in &self.entries {
            body.extend_from_slice(&entry.epoch.dimension.0.to_le_bytes());
            body.extend_from_slice(&entry.epoch.id.0.to_le_bytes());
            body.push(match entry.state {
                EpochState::Established => ESTABLISHED,
                EpochState::Candidate => CANDIDATE,
            });
            body.extend_from_slice(&entry.last_used.get().to_le_bytes());
        }
        seal(CATALOG_MAGIC, CATALOG_FORMAT_VERSION, &body)
    }

    /// Reads [`Self::encode`] output, rejecting bad framing, unknown states, repeated
    /// epochs and ids at or past the next id.
    pub fn decode(bytes: &[u8]) -> Result<Self, TableError> {
        let mut reader = BodyReader::new(unseal(CATALOG_MAGIC, CATALOG_FORMAT_VERSION, bytes)?);
        let next_id = reader.u32()?;
        let count = reader.count(ENTRY_LEN)?;
        let mut catalog = Self {
            next_id,
            entries: Vec::with_capacity(count),
        };
        for _ in 0..count {
            let epoch = EpochRef {
                dimension: Dimension(reader.i32()?),
                id: EpochId(reader.u32()?),
            };
            let state = match reader.u8()? {
                ESTABLISHED => EpochState::Established,
                CANDIDATE => EpochState::Candidate,
                _ => return Err(TableError::Body("unknown epoch state")),
            };
            let last_used = Minute::new(reader.u32()?);
            if epoch.id.0 >= next_id
                || catalog
                    .entries
                    .iter()
                    .any(|entry| entry.epoch.id == epoch.id)
            {
                return Err(TableError::Body("epoch id repeated or not yet assigned"));
            }
            catalog.entries.push(EpochEntry {
                epoch,
                state,
                last_used,
            });
        }
        reader.finish()?;
        Ok(catalog)
    }

    /// The mutable entry of `epoch`; `None` when it is not catalogued.
    fn entry_mut(&mut self, epoch: EpochRef) -> Option<&mut EpochEntry> {
        self.entries.iter_mut().find(|entry| entry.epoch == epoch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn overworld(id: u32) -> EpochRef {
        EpochRef {
            dimension: Dimension(0),
            id: EpochId(id),
        }
    }

    #[test]
    fn catalogs_round_trip_and_reject_bad_entries() {
        let mut catalog = EpochCatalog::new();
        let first = catalog
            .create(Dimension(0), EpochState::Established, Minute::new(5))
            .expect("id");
        let second = catalog
            .create(Dimension(-1), EpochState::Candidate, Minute::new(6))
            .expect("id");
        assert_ne!(first.id, second.id);
        let encoded = catalog.encode();
        assert_eq!(EpochCatalog::decode(&encoded), Ok(catalog.clone()));

        let mut duplicate = catalog.clone();
        duplicate.entries.push(duplicate.entries[0]);
        assert!(EpochCatalog::decode(&duplicate.encode()).is_err());
        let mut unassigned = catalog;
        unassigned.next_id = 1;
        assert!(EpochCatalog::decode(&unassigned.encode()).is_err());
    }

    #[test]
    fn the_least_recently_used_epoch_outside_keep_is_evicted_past_the_limit() {
        let mut catalog = EpochCatalog::new();
        for minute in 0..MAX_EPOCHS_PER_STORE as u32 {
            catalog.create(Dimension(0), EpochState::Established, Minute::new(minute));
        }
        catalog.create(Dimension(0), EpochState::Candidate, Minute::new(0));
        assert_eq!(catalog.over_limit(&[]), None, "candidates do not count");
        catalog.create(Dimension(0), EpochState::Established, Minute::new(100));
        assert_eq!(catalog.over_limit(&[]), Some(overworld(0)));
        assert_eq!(catalog.over_limit(&[overworld(0)]), Some(overworld(1)));
        catalog.touch(overworld(0), Minute::new(200));
        assert_eq!(catalog.over_limit(&[]), Some(overworld(1)));
    }

    #[test]
    fn rebuilt_catalogs_never_reuse_ids_and_names_round_trip() {
        let catalog =
            EpochCatalog::rebuilt([overworld(4), overworld(1), overworld(4)], Minute::new(9));
        assert_eq!(catalog.entries().len(), 2);
        let mut catalog = catalog;
        let next = catalog
            .create(Dimension(0), EpochState::Candidate, Minute::new(9))
            .expect("id");
        assert_eq!(next.id, EpochId(5));
        assert_eq!(
            EpochId::from_dir_name(&EpochId(12).dir_name()),
            Some(EpochId(12))
        );
        assert_eq!(EpochId::from_dir_name("e01"), None);
        assert_eq!(EpochId::from_dir_name("dim0"), None);
    }
}
