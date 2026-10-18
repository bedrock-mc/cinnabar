use crate::chunk::*;
use render_scene::{ChunkUploadAcknowledgements, ChunkUploadBudget};

/// Returns whether retirement capacity deferred a resident removal this frame.
/// The independent zero-byte operation allowance is not retirement pressure.
pub(super) fn prepare_publication_removals(
    arena: &mut ChunkGpuArena,
    budget: ChunkUploadBudget,
    gpu_removals: &ChunkGpuRemovalQueue,
    acknowledgements: &ChunkUploadAcknowledgements,
    mut terrain_generations: Option<
        &mut crate::dropped_item_render::terrain_items::TerrainItemMeshGenerations,
    >,
) -> bool {
    let mut retirement_pressure = false;
    let maximum_zero_byte_operations = budget
        .max_zero_byte_operations_per_frame
        .min(PublicationServiceConfig::DEFAULT.maximum_zero_byte_operations_per_frame);
    let mut zero_byte_operations = 0;
    let mut physically_removed_keys = HashSet::new();
    let urgent_removal_keys = gpu_removals.urgent_keys();
    let mut pending_removal_entities = arena.pending_removals.iter().copied().collect::<Vec<_>>();
    pending_removal_entities.sort_by_key(|entity| {
        arena
            .allocations
            .get(entity)
            .is_none_or(|allocation| !urgent_removal_keys.contains(&allocation.gpu.key))
    });
    for entity in pending_removal_entities {
        let Some(allocation) = arena.allocations.get(&entity).cloned() else {
            arena.pending_removals.remove(&entity);
            continue;
        };
        if zero_byte_operations >= maximum_zero_byte_operations {
            break;
        }
        let allocation_key = allocation.gpu.key;
        // Opaque buffers can still be read by a submitted frame just like
        // liquid buffers. Retire every allocation behind the same completion
        // fence so a removal cannot recycle its ranges while the GPU is using
        // them.
        let retirement = RetiredArenaAllocation::full(entity, allocation);
        let bytes = retirement.owned_bytes();
        if !arena.retirement_budget.can_reserve(1, bytes) {
            retirement_pressure = true;
            continue;
        }
        arena
            .allocations
            .remove(&entity)
            .expect("pending removal retains its arena allocation");
        arena.transparent_liquids.forget(entity, allocation_key);
        physically_removed_keys.insert(allocation_key);
        assert!(arena.retirement_budget.try_reserve(1, bytes));
        arena.retired_allocations.push(retirement);
        arena.pending_removals.remove(&entity);
        zero_byte_operations = zero_byte_operations.saturating_add(1);
    }

    let mut live_keys = LiveAllocationKeys::default();
    let completed_removals = gpu_removals.take_ready(maximum_zero_byte_operations, |key| {
        if physically_removed_keys.contains(&key) {
            return true;
        }
        if zero_byte_operations >= maximum_zero_byte_operations {
            return false;
        }
        if live_keys.contains(
            key,
            arena
                .allocations
                .values()
                .map(|allocation| allocation.gpu.key),
        ) {
            return false;
        }
        zero_byte_operations = zero_byte_operations.saturating_add(1);
        true
    });
    for pending in completed_removals {
        if pending
            .token
            .is_some_and(|token| !acknowledgements.try_reserve(pending.key, token))
        {
            gpu_removals
                .requeue(pending)
                .unwrap_or_else(|_| unreachable!("taking one removal frees one mailbox slot"));
            continue;
        }
        let permit = match pending.permit.into_gpu_prepared() {
            Ok(permit) => permit,
            Err(permit) => {
                if let Some(token) = pending.token {
                    acknowledgements.cancel(pending.key, token);
                }
                drop(permit);
                continue;
            }
        };
        if let Some(token) = pending.token {
            if let Some(source) = &pending.publications {
                source.record(
                    pending.key,
                    token.generation,
                    pending.kind == render_api::TerrainRemovalKind::Empty,
                );
            }
            acknowledgements.complete(pending.key, token, Instant::now());
            if let Some(terrain) = terrain_generations.as_deref_mut() {
                terrain.record(pending.key, token.generation);
            }
        }
        let retired = permit.retire();
        debug_assert!(retired);
    }
    retirement_pressure
}

/// Uses direct lookups for small passes and shares an index once scans cost a full arena.
#[derive(Default)]
struct LiveAllocationKeys {
    cached: Option<HashSet<SubChunkKey>>,
    linear_visits: usize,
}

impl LiveAllocationKeys {
    /// Indexes the immutable post-retirement keys only after earlier scans visited their count.
    fn contains(
        &mut self,
        key: SubChunkKey,
        mut allocation_keys: impl ExactSizeIterator<Item = SubChunkKey>,
    ) -> bool {
        if let Some(cached) = &self.cached {
            return cached.contains(&key);
        }
        if self.linear_visits >= allocation_keys.len() {
            let cached = self.cached.insert(allocation_keys.collect());
            return cached.contains(&key);
        }
        let before = allocation_keys.len();
        let found = allocation_keys.any(|candidate| candidate == key);
        self.linear_visits = self
            .linear_visits
            .saturating_add(before - allocation_keys.len());
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// A single receipt retains the early-exit lookup and allocates no index.
    #[test]
    fn one_receipt_keeps_its_direct_lookup() {
        let keys = (0..1_024)
            .map(|x| SubChunkKey::new(0, x, 0, 0))
            .collect::<Vec<_>>();
        for (key, expected_reads, present) in [
            (keys[0], 1, true),
            (SubChunkKey::new(0, 1_024, 0, 0), keys.len(), false),
        ] {
            let reads = Cell::new(0);
            let mut lookup = LiveAllocationKeys::default();
            assert_eq!(
                lookup.contains(
                    key,
                    keys.iter().copied().inspect(|_| reads.set(reads.get() + 1)),
                ),
                present,
            );
            assert_eq!(reads.get(), expected_reads);
            assert!(lookup.cached.is_none());
        }
    }

    /// Repeated absent receipts spend one full scan and one shared index construction.
    #[test]
    fn air_receipts_share_an_index_after_one_full_scan() {
        let keys = (0..1_024)
            .map(|x| SubChunkKey::new(0, x, 0, 0))
            .collect::<Vec<_>>();
        let reads = Cell::new(0);
        let mut lookup = LiveAllocationKeys::default();
        for x in 1_024..1_280 {
            assert!(!lookup.contains(
                SubChunkKey::new(0, x, 0, 0),
                keys.iter().copied().inspect(|_| reads.set(reads.get() + 1)),
            ));
        }
        assert!(lookup.contains(
            keys[0],
            keys.iter().copied().inspect(|_| reads.set(reads.get() + 1)),
        ));
        assert_eq!(reads.get(), keys.len() * 2);
    }

    /// A scan crossing the arena-sized work threshold still bounds total key visits linearly.
    #[test]
    fn mixed_receipts_bound_scans_before_indexing() {
        let keys = (0..1_024)
            .map(|x| SubChunkKey::new(0, x, 0, 0))
            .collect::<Vec<_>>();
        let reads = Cell::new(0);
        let mut lookup = LiveAllocationKeys::default();
        for key in
            std::iter::once(keys[0]).chain((1_024..1_280).map(|x| SubChunkKey::new(0, x, 0, 0)))
        {
            assert_eq!(
                lookup.contains(
                    key,
                    keys.iter().copied().inspect(|_| reads.set(reads.get() + 1)),
                ),
                key == keys[0],
            );
        }
        assert!(reads.get() <= keys.len() * 3);
    }
}
