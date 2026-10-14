//! The sections a writer reads, through a bounded cache in front of the store.

use std::collections::HashMap;

use lod::{
    codec::SectionCodec,
    geometry::{SectionKey, SectionPos},
    ingest::SectionSource,
    section::Section,
};
use lod_store::{
    actor::{ReadOutcome, ReadTarget, StoreClient, SubmitError},
    identity::Dimension,
    minute::Minute,
};

/// Why a merge could not read a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) enum LoadError {
    /// The store's queue is full; retry later.
    Full,
    /// The store failed or stopped.
    Failed,
}

/// The store's sections of one dimension, read through one writer's cache. The cache
/// holds every write of the writer's own sections; another writer's section is read
/// again once that writer hands over a slab facing it, which it does after storing any
/// change to that face, and the store answers a read after every write sent before it.
pub(in crate::stream) struct StoreSections<'a> {
    pub(in crate::stream) client: &'a StoreClient,
    pub(in crate::stream) cache: &'a mut SectionCache,
    pub(in crate::stream) codec: &'a mut SectionCodec,
    pub(in crate::stream) dimension: Dimension,
    pub(in crate::stream) now: Minute,
}

impl SectionSource for StoreSections<'_> {
    type Error = LoadError;

    /// Cached sections, else the store's; corrupt records read as missing so the merge
    /// rebuilds them.
    fn load(&mut self, pos: SectionPos) -> Result<Option<Section>, LoadError> {
        let key = pos.key();
        if let Some(section) = self.cache.get(self.dimension, key) {
            return Ok(section);
        }
        let outcome = self
            .client
            .read(self.dimension, ReadTarget::OwnSection(key), self.now)
            .map_err(|error| match error {
                SubmitError::Full(()) => LoadError::Full,
                SubmitError::Stopped(()) => LoadError::Failed,
            })?;
        let section = match outcome {
            ReadOutcome::Found(record) => self.codec.decode_expecting(&record, key).ok(),
            ReadOutcome::Missing | ReadOutcome::Corrupt => None,
            ReadOutcome::ChunkHash(_) | ReadOutcome::Failed(_) => return Err(LoadError::Failed),
        };
        self.cache.insert(self.dimension, key, section.clone());
        Ok(section)
    }
}

/// Bytes counted for each cached entry besides its section.
const ENTRY_OVERHEAD_BYTES: usize = 64;

/// Recently read or written sections by dimension and key, `None` for known misses,
/// bounded in bytes with the least recently used dropped first.
#[derive(Default)]
pub(in crate::stream) struct SectionCache {
    entries: HashMap<(Dimension, SectionKey), (u64, Option<Section>)>,
    tick: u64,
    bytes: usize,
    budget: usize,
}

/// Bytes one cached entry holds.
fn entry_bytes(section: Option<&Section>) -> usize {
    ENTRY_OVERHEAD_BYTES
        + section.map_or(0, |section| {
            size_of::<Section>() + section.voxels().heap_bytes()
        })
}

impl SectionCache {
    /// An empty cache keeping about `budget` bytes of sections.
    pub(in crate::stream) fn new(budget: usize) -> Self {
        Self {
            budget,
            ..Self::default()
        }
    }

    /// The cached state of `key`; `None` when it is not cached.
    fn get(&mut self, dimension: Dimension, key: SectionKey) -> Option<Option<Section>> {
        self.tick += 1;
        let entry = self.entries.get_mut(&(dimension, key))?;
        entry.0 = self.tick;
        Some(entry.1.clone())
    }

    /// Caches `section` for `key`; beyond the budget, drops the least recently used
    /// entries until three quarters of it are left.
    pub(in crate::stream) fn insert(
        &mut self,
        dimension: Dimension,
        key: SectionKey,
        section: Option<Section>,
    ) {
        self.tick += 1;
        self.forget(dimension, key);
        self.bytes += entry_bytes(section.as_ref());
        self.entries.insert((dimension, key), (self.tick, section));
        if self.bytes <= self.budget {
            return;
        }
        let mut by_age: Vec<(u64, (Dimension, SectionKey))> = self
            .entries
            .iter()
            .map(|(&entry, (tick, _))| (*tick, entry))
            .collect();
        by_age.sort_unstable_by_key(|(tick, _)| *tick);
        for (_, (dimension, key)) in by_age {
            if self.bytes <= self.budget / 4 * 3 {
                break;
            }
            self.forget(dimension, key);
        }
    }

    /// Forgets the section of `key`, so it is read from the store again.
    pub(in crate::stream) fn forget(&mut self, dimension: Dimension, key: SectionKey) {
        if let Some((_, section)) = self.entries.remove(&(dimension, key)) {
            self.bytes -= entry_bytes(section.as_ref());
        }
    }

    /// Forgets every section.
    pub(in crate::stream) fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }
}
