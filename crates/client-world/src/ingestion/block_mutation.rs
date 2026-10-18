//! Primary block transitions and their implicit secondary water updates.

use assets::{NetworkIdMode, facts::liquid::pinned_facts};
use hashbrown::HashMap;
use world::{
    BlockUpdate, ChunkStore, MutationError, PreparedSubChunkMutation, SubChunk, SubChunkKey,
};

/// Prepares ordered primary/secondary writes, including water retained by an admitted block.
pub fn prepare_block_mutation(
    key: SubChunkKey,
    previous: Option<&SubChunk>,
    updates: &[BlockUpdate],
    air: u32,
    mode: NetworkIdMode,
) -> Result<PreparedSubChunkMutation, MutationError> {
    for update in updates {
        update.validate()?;
    }
    let mut cells = HashMap::<[u8; 3], [u32; 2]>::new();
    let mut expanded = Vec::with_capacity(updates.len());
    for &update in updates {
        if update.layer > 1 {
            expanded.push(update);
            continue;
        }
        let cell = [update.x, update.y, update.z];
        let layers = cells.entry(cell).or_insert_with(|| {
            std::array::from_fn(|layer| {
                previous
                    .and_then(|chunk| chunk.runtime_id(layer, cell[0], cell[1], cell[2]))
                    .unwrap_or(air)
            })
        });
        if update.layer == 0
            && let Some(next) = pinned_facts(mode, update.runtime_id)
        {
            let secondary = if !next.can_contain_liquid_source
                && pinned_facts(mode, layers[1]).is_some_and(|facts| facts.water)
            {
                Some(air)
            } else if next.can_contain_liquid_source
                && !next.air
                && layers[1] == air
                && pinned_facts(mode, layers[0]).is_some_and(|facts| facts.water_source)
            {
                Some(layers[0])
            } else {
                None
            };
            if let Some(runtime_id) = secondary {
                layers[1] = runtime_id;
                expanded.push(BlockUpdate {
                    layer: 1,
                    runtime_id,
                    ..update
                });
            }
        }
        layers[update.layer as usize] = update.runtime_id;
        expanded.push(update);
    }
    ChunkStore::prepare_sub_chunk_blocks(key, previous, &expanded, air)
}
