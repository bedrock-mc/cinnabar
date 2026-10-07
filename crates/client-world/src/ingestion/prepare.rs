//! Pure payload and block mutation preparation, independent of rendering.

use assets::{NetworkIdMode, RuntimeAssets};
use hashbrown::HashMap as FastHashMap;
use protocol::{SubChunkBatchEvent, SubChunkResult};
use std::collections::BTreeSet;
use world::{BlockIds, ChunkStore, DecodedSubChunk, MutationError, SubChunk, SubChunkKey};

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
    };
    for batch in batches {
        let mutation = ChunkStore::prepare_sub_chunk_blocks(
            batch.key,
            batch.previous.as_deref(),
            &batch.updates,
            ids.air(),
        )?;
        if mutation.changed()
            && light_semantics_changed(
                ids.air,
                &ids.assets,
                ids.mode,
                batch.previous.as_deref(),
                mutation.replacement(),
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

/// Compares the exact block-light inputs against an immutable registry snapshot.
pub fn light_semantics_changed(
    air_network_id: u32,
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    previous: Option<&SubChunk>,
    replacement: Option<&SubChunk>,
) -> bool {
    if previous.is_some() != replacement.is_some() {
        return true;
    }
    let mut resolved = FastHashMap::<u32, (u8, u8)>::new();
    for y in 0..16 {
        for z in 0..16 {
            for x in 0..16 {
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
                                    let properties =
                                        assets.resolve(mode, runtime_id).light_properties();
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
        }
    }
    false
}
