//! Bounded reuse of records built from immutable biome and tint sources.

use std::{collections::VecDeque, sync::Mutex};

use super::super::*;

const CACHE_BYTES: u64 = 8 * 1024 * 1024;
const CACHE_ENTRIES: usize = 1024;

type SourceKey = [SlotIdentity; BIOME_NEIGHBOUR_SLOT_COUNT];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SlotIdentity {
    Missing,
    Uniform(u32),
    Packed(usize),
}

struct Entry {
    record: PackedBiomeRecord,
    /// Keeping mixed sources alive prevents address reuse while the entry exists.
    _sources: Vec<Arc<BiomeStorage>>,
    bytes: u64,
}

#[derive(Default)]
struct Entries {
    records: HashMap<SourceKey, Entry>,
    order: VecDeque<SourceKey>,
    bytes: u64,
}

/// A cache belongs to one dimension, pack allocation, and exact resolved tint snapshot.
/// Old jobs retain their old cache after reloads; they cannot populate the new one.
pub(in crate::stream) struct BiomeRecordCache {
    dimension: i32,
    tint_identity: ChunkBiomeTintIdentity,
    assets: Arc<RuntimeAssets>,
    resolved: Arc<ResolvedBiomeTints>,
    entries: Mutex<Entries>,
}

impl BiomeRecordCache {
    /// Starts an empty namespace for the job's immutable source generation.
    fn new(
        dimension: i32,
        tint_identity: ChunkBiomeTintIdentity,
        assets: Arc<RuntimeAssets>,
        resolved: Arc<ResolvedBiomeTints>,
    ) -> Self {
        Self {
            dimension,
            tint_identity,
            assets,
            resolved,
            entries: Mutex::default(),
        }
    }

    /// Checks identities rather than colour equality, including every pack replacement.
    fn matches(
        &self,
        dimension: i32,
        tint_identity: ChunkBiomeTintIdentity,
        assets: &Arc<RuntimeAssets>,
        resolved: &Arc<ResolvedBiomeTints>,
    ) -> bool {
        self.dimension == dimension
            && self.tint_identity == tint_identity
            && Arc::ptr_eq(&self.assets, assets)
            && Arc::ptr_eq(&self.resolved, resolved)
    }

    /// Returns a shared record on an exact hit; misses build outside the cache lock.
    pub(in crate::stream) fn record(&self, sources: &BiomeNeighbourhood) -> PackedBiomeRecord {
        #[cfg(feature = "benchmark-support")]
        let started = Instant::now();
        let key = sources.each_ref().map(|source| match source {
            None => SlotIdentity::Missing,
            Some(storage) if storage.bits_per_index() == 0 => {
                SlotIdentity::Uniform(storage.palette().values()[0])
            }
            Some(storage) => SlotIdentity::Packed(Arc::as_ptr(storage) as usize),
        });
        let known = self
            .entries
            .lock()
            .expect("biome cache lock poisoned")
            .records
            .get(&key)
            .map(|entry| entry.record.clone());
        let record = if let Some(record) = known {
            #[cfg(feature = "benchmark-support")]
            super::super::benchmark_support::biomes::observe_cache_hit();
            record
        } else {
            let record = build_record(sources, &self.resolved);
            let retained: Vec<_> = sources
                .iter()
                .flatten()
                .filter(|source| source.bits_per_index() != 0)
                .cloned()
                .collect();
            // Count every retained source conservatively, including repeated slots.
            let bytes = record.byte_len()
                + retained
                    .iter()
                    .map(|source| {
                        (size_of::<BiomeStorage>()
                            + size_of::<Arc<BiomeStorage>>()
                            + size_of::<SlotIdentity>()
                            + 2 * size_of::<usize>()
                            + source.retained_heap_bytes_bound()) as u64
                    })
                    .sum::<u64>()
                + size_of::<Entry>() as u64
                + size_of::<SourceKey>() as u64 * 2;
            let mut entries = self.entries.lock().expect("biome cache lock poisoned");
            if let Some(known) = entries.records.get(&key) {
                known.record.clone()
            } else if bytes > CACHE_BYTES {
                record
            } else {
                while entries.bytes + bytes > CACHE_BYTES || entries.records.len() >= CACHE_ENTRIES
                {
                    let oldest = entries
                        .order
                        .pop_front()
                        .expect("bounded cache has an oldest entry");
                    let retired = entries
                        .records
                        .remove(&oldest)
                        .expect("cache order tracks each entry");
                    entries.bytes -= retired.bytes;
                }
                entries.bytes += bytes;
                entries.order.push_back(key);
                entries.records.insert(
                    key,
                    Entry {
                        record: record.clone(),
                        _sources: retained,
                        bytes,
                    },
                );
                record
            }
        };
        #[cfg(feature = "benchmark-support")]
        super::super::benchmark_support::biomes::observe_record(started.elapsed());
        record
    }
}

impl WorldStream {
    /// Switches namespaces before dispatch so stale workers retain only the previous cache.
    pub(in crate::stream) fn biome_record_cache(
        &mut self,
        dimension: i32,
    ) -> Arc<BiomeRecordCache> {
        let identity = self.biome_tint_identity();
        let assets = self.authority.runtime_assets();
        let resolved = self.authority.resolved_biome_tints();
        if self
            .biome_records
            .as_ref()
            .is_none_or(|cache| !cache.matches(dimension, identity, assets, resolved))
        {
            self.biome_records = Some(Arc::new(BiomeRecordCache::new(
                dimension,
                identity,
                assets.clone(),
                resolved.clone(),
            )));
        }
        self.biome_records
            .as_ref()
            .expect("current biome cache installed")
            .clone()
    }
}

/// Builds a record without cache bookkeeping for standalone snapshots and validation callers.
pub(in crate::stream) fn pack_biome_record(
    sources: &BiomeNeighbourhood,
    resolved: &ResolvedBiomeTints,
) -> PackedBiomeRecord {
    #[cfg(feature = "benchmark-support")]
    let started = Instant::now();
    let record = build_record(sources, resolved);
    #[cfg(feature = "benchmark-support")]
    super::super::benchmark_support::biomes::observe_record(started.elapsed());
    record
}

/// Resolves only palette IDs; packed voxel words remain unchanged in the record.
fn build_record(sources: &BiomeNeighbourhood, resolved: &ResolvedBiomeTints) -> PackedBiomeRecord {
    PackedBiomeRecord::from_neighbourhood(sources, |raw_id| resolved.dense_index(raw_id))
}

#[cfg(test)]
mod tests;
