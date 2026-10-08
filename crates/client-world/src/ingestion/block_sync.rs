use std::collections::BTreeMap;

use protocol::SyncedBlockUpdateEvent;
use world::{MutationError, SUB_CHUNK_SIDE, SubChunkKey};

use super::{
    BlockMutationBatch, DecodeIds, PreparedBlockMutations,
    prepare::{prepare_resolved_block_mutations, resolve_block_mutations},
};

pub(super) fn prepare_synced_block_mutations(
    mut batches: Vec<BlockMutationBatch>,
    mut events: Vec<SyncedBlockUpdateEvent>,
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
    (prepare_resolved_block_mutations(batches, ids), events)
}
