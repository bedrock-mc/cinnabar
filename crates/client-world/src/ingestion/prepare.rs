//! Pure payload and block mutation preparation, independent of rendering.

use assets::{NetworkIdMode, RuntimeAssets};
use hashbrown::HashMap as FastHashMap;
use protocol::{SubChunkBatchEvent, SubChunkResult};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use world::{BlockIds, DecodedLevelChunk, DecodedSubChunk, MutationError, SubChunk, SubChunkKey};

use super::{
    BlockMutationBatch, DecodeIds, PreparedBlockMutations, PreparedSubChunk, PreparedSubChunkResult,
};

/// Prepares packed mutations and the same light comparison used by direct predictions.
pub fn prepare_block_mutations(
    mut batches: Vec<BlockMutationBatch>,
    ids: &DecodeIds,
) -> Result<PreparedBlockMutations, MutationError> {
    resolve_block_mutations(&mut batches, ids);
    prepare_resolved_block_mutations(batches, ids)
}

pub(super) fn resolve_block_mutations(batches: &mut [BlockMutationBatch], ids: &DecodeIds) {
    for batch in batches {
        for update in &mut batch.updates {
            update.runtime_id = BlockIds::resolve(ids, update.runtime_id);
        }
    }
}

pub(super) fn prepare_resolved_block_mutations(
    batches: Vec<BlockMutationBatch>,
    ids: &DecodeIds,
) -> Result<PreparedBlockMutations, MutationError> {
    let mut prepared = PreparedBlockMutations {
        mutations: Vec::with_capacity(batches.len()),
        relight: BTreeSet::new(),
        crack_intermediates: Vec::new(),
    };
    for batch in batches {
        let mutation = super::prepare_block_mutation(
            batch.key,
            batch.previous.as_deref(),
            &batch.updates,
            ids.air,
            ids.mode,
        )?;
        if mutation.changed()
            && light_semantics_changed_at(
                ids.air,
                &ids.assets,
                ids.mode,
                batch.previous.as_deref(),
                mutation.replacement(),
                mutation.changed_positions().map(|position| {
                    [position.x, position.y, position.z]
                        .map(|coordinate| coordinate.rem_euclid(world::SUB_CHUNK_SIDE as i32) as u8)
                }),
            )
        {
            prepared.relight.insert(mutation.key());
        }
        prepared.mutations.push(mutation);
    }
    Ok(prepared)
}

/// Decodes a subchunk batch against the session's immutable registry snapshot.
pub fn prepare_sub_chunks(batch: SubChunkBatchEvent, ids: &DecodeIds) -> Vec<PreparedSubChunk> {
    let dimension = batch.dimension;
    batch
        .entries
        .into_iter()
        .map(|entry| {
            let key = SubChunkKey::new(
                dimension,
                entry.position[0],
                entry.position[1],
                entry.position[2],
            );
            PreparedSubChunk {
                position: entry.position,
                diagnostics: entry.diagnostics,
                result: match entry.result {
                    SubChunkResult::Success { payload } => {
                        PreparedSubChunkResult::Decoded(DecodedSubChunk::decode(key, &payload, ids))
                    }
                    SubChunkResult::AllAir => PreparedSubChunkResult::AllAir,
                    SubChunkResult::Unavailable(unavailable) => {
                        PreparedSubChunkResult::Unavailable(unavailable)
                    }
                },
            }
        })
        .collect()
}

/// Sections of a resent column whose blocks differ from `previous` while every cell keeps its
/// light inputs, each paired with the resident section it was compared with.
pub fn light_neutral_sections(
    previous: &BTreeMap<i32, Arc<SubChunk>>,
    decoded: &DecodedLevelChunk,
    ids: &DecodeIds,
) -> Vec<(i32, Arc<SubChunk>)> {
    decoded
        .sub_chunks()
        .filter_map(|(y, replacement)| {
            let previous = previous.get(&y)?;
            (!Arc::ptr_eq(previous, &replacement)
                && !previous.blocks_match(&replacement)
                && !light_semantics_changed(
                    ids.air,
                    &ids.assets,
                    ids.mode,
                    Some(previous.as_ref()),
                    Some(replacement.as_ref()),
                ))
            .then(|| (y, Arc::clone(previous)))
        })
        .collect()
}

/// Compares the exact block-light inputs against an immutable registry snapshot.
pub fn light_semantics_changed(
    air_network_id: u32,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    previous: Option<&SubChunk>,
    replacement: Option<&SubChunk>,
) -> bool {
    light_semantics_changed_at(
        air_network_id,
        assets,
        mode,
        previous,
        replacement,
        (0_u8..16).flat_map(|y| (0_u8..16).flat_map(move |z| (0_u8..16).map(move |x| [x, y, z]))),
    )
}

/// Compares combined layer inputs only at cells this mutation could have changed.
fn light_semantics_changed_at(
    air_network_id: u32,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    previous: Option<&SubChunk>,
    replacement: Option<&SubChunk>,
    positions: impl IntoIterator<Item = [u8; 3]>,
) -> bool {
    if previous.is_some() != replacement.is_some() {
        return true;
    }
    let mut resolved = FastHashMap::<u32, (u8, u8)>::new();
    for [x, y, z] in positions {
        let mut sample = |sub_chunk: Option<&SubChunk>| {
            let mut emission = 0;
            let mut has_non_air = false;
            let mut filter = 0;
            if let Some(sub_chunk) = sub_chunk {
                for layer in 0..sub_chunk.storages().len() {
                    let Some(runtime_id) = sub_chunk.runtime_id(layer, x, y, z) else {
                        continue;
                    };
                    if runtime_id == air_network_id {
                        continue;
                    }
                    has_non_air = true;
                    let (block_emission, block_filter) =
                        *resolved.entry(runtime_id).or_insert_with(|| {
                            let properties = assets.resolve(mode, runtime_id).light_properties();
                            (properties.emission(), properties.filter())
                        });
                    emission = emission.max(block_emission);
                    filter = filter.max(block_filter);
                }
            }
            (has_non_air, emission, filter)
        };
        if sample(previous) != sample(replacement) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use world::{BlockUpdate, ChunkStore, RawBlockIds};

    /// Builds a small decoded section with explicit primary and secondary layers.
    fn section(layers: &[u32]) -> SubChunk {
        let mut payload = vec![9, layers.len() as u8, 0];
        for &id in layers {
            payload.extend([1, (id << 1) as u8]);
        }
        SubChunk::decode(&payload, &RawBlockIds { air: 0 })
    }

    #[test]
    fn changed_cell_light_comparison_matches_full_section_for_layer_and_presence_changes() {
        let assets = RuntimeAssets::diagnostic();
        let key = SubChunkKey::new(0, -2, 0, -3);
        for previous in [
            None,
            Some(section(&[1])),
            Some(section(&[1, 0])),
            Some(section(&[0, 1])),
        ] {
            for updates in [
                vec![BlockUpdate::new(1, 2, 3, 0, 0)],
                vec![BlockUpdate::new(1, 2, 3, 1, 1)],
                vec![BlockUpdate::new(15, 15, 15, 0, 2)],
                vec![
                    BlockUpdate::new(0, 0, 0, 0, 2),
                    BlockUpdate::new(0, 0, 0, 0, 1),
                ],
                vec![
                    BlockUpdate::new(0, 0, 0, 0, 0),
                    BlockUpdate::new(0, 0, 0, 1, 1),
                ],
            ] {
                let mutation =
                    ChunkStore::prepare_sub_chunk_blocks(key, previous.as_ref(), &updates, 0)
                        .unwrap();
                let full = light_semantics_changed(
                    0,
                    &assets,
                    NetworkIdMode::Sequential,
                    previous.as_ref(),
                    mutation.replacement(),
                );
                let sparse = light_semantics_changed_at(
                    0,
                    &assets,
                    NetworkIdMode::Sequential,
                    previous.as_ref(),
                    mutation.replacement(),
                    mutation.changed_positions().map(|position| {
                        [position.x, position.y, position.z].map(|coordinate| {
                            coordinate.rem_euclid(world::SUB_CHUNK_SIDE as i32) as u8
                        })
                    }),
                );
                assert_eq!(
                    sparse,
                    full,
                    "layers={:?}, updates={updates:?}",
                    previous.as_ref().map(|section| section.storages().len())
                );
            }
        }
    }

    #[test]
    fn light_neutral_comparison_visits_only_supplied_cells() {
        let assets = RuntimeAssets::diagnostic();
        let section = section(&[1]);
        let mut visited = 0;
        let positions = [[1, 2, 3], [15, 15, 15]]
            .into_iter()
            .inspect(|_| visited += 1);
        assert!(!light_semantics_changed_at(
            0,
            &assets,
            NetworkIdMode::Sequential,
            Some(&section),
            Some(&section),
            positions
        ));
        assert_eq!(visited, 2);
    }
}
