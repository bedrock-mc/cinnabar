use std::{collections::BTreeMap, sync::Arc};

use protocol::SyncedBlockUpdateEvent;
use world::{MutationError, SUB_CHUNK_SIDE, SubChunkKey};

use super::{
    BlockMutationBatch, DecodeIds, PreparedBlockMutations,
    prepare::{prepare_resolved_block_mutations, resolve_block_mutations},
};

/// Prepares the final terrain and retains earlier cracked-section states and actor transitions.
pub(super) fn prepare_synced_block_mutations(
    mut batches: Vec<BlockMutationBatch>,
    mut events: Vec<SyncedBlockUpdateEvent>,
    mut crack_batches: Vec<BlockMutationBatch>,
    ids: &DecodeIds,
) -> (
    Result<PreparedBlockMutations, MutationError>,
    Vec<SyncedBlockUpdateEvent>,
) {
    resolve_block_mutations(&mut batches, ids);
    let mut grouped = batches
        .iter()
        .map(|batch| (batch.key, (batch, 0usize)))
        .collect::<BTreeMap<_, _>>();
    let mut current = BTreeMap::new();
    events.retain(|event| {
        let [x, y, z] = event.update.position;
        let side = SUB_CHUNK_SIDE as i32;
        let key = SubChunkKey::new(
            event.update.dimension,
            x.div_euclid(side),
            y.div_euclid(side),
            z.div_euclid(side),
        );
        let Some((batch, next)) = grouped.get_mut(&key) else {
            return false;
        };
        let Some(update) = batch.updates.get(*next) else {
            return false;
        };
        *next += 1;
        let cell = (key, update.layer, update.x, update.y, update.z);
        let previous = current.insert(cell, update.runtime_id).unwrap_or_else(|| {
            batch
                .previous
                .as_deref()
                .and_then(|chunk| {
                    chunk.runtime_id(update.layer as usize, update.x, update.y, update.z)
                })
                .unwrap_or(ids.air)
        });
        // Writes enter the terrain store independently of render notifications.
        event.update.layer == 0
            && event.flags & 6 == 2
            && (event.flags & 0x10 != 0 || previous != update.runtime_id)
    });
    let result = prepare_resolved_block_mutations(batches, ids).and_then(|mut prepared| {
        resolve_block_mutations(&mut crack_batches, ids);
        let mut previous = BTreeMap::new();
        for batch in crack_batches {
            let section = previous.entry(batch.key).or_insert(batch.previous);
            let mutation = super::prepare_block_mutation(
                batch.key,
                section.as_deref(),
                &batch.updates,
                ids.air,
                ids.mode,
            )?;
            *section = mutation.replacement().cloned().map(Arc::new);
            prepared.crack_intermediates.push(mutation);
        }
        Ok(prepared)
    });
    (result, events)
}
