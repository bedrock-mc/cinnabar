use super::*;

pub(super) fn ensure_chunk_block_entity_bytes(bytes: usize) -> Result<(), BlockEntityError> {
    if bytes > MAX_BLOCK_ENTITY_BYTES_PER_CHUNK {
        Err(BlockEntityError::ChunkEntityBytesTooLarge {
            len: bytes,
            max: MAX_BLOCK_ENTITY_BYTES_PER_CHUNK,
        })
    } else {
        Ok(())
    }
}

pub(super) fn reuse_equal_biome_arcs(
    replacement: &mut DecodedBiomeColumn,
    previous: Option<&DecodedBiomeColumn>,
) {
    let Some(previous) = previous else {
        return;
    };
    for (offset, storage) in replacement.storages.iter_mut().enumerate() {
        let Some(y) = replacement
            .base_sub_chunk_y
            .checked_add(i32::try_from(offset).expect("biome columns are bounded"))
        else {
            continue;
        };
        let Some(previous) = previous.storage(y) else {
            continue;
        };
        if previous.as_ref() == storage.as_ref() {
            *storage = previous;
        }
    }
}

pub(super) fn changed_biome_ys(
    previous: Option<&DecodedBiomeColumn>,
    replacement: Option<&DecodedBiomeColumn>,
) -> BTreeSet<i32> {
    let ys = |column: &DecodedBiomeColumn| {
        let base = column.base_sub_chunk_y();
        let len = column.len();
        (0..len).filter_map(move |offset| base.checked_add(i32::try_from(offset).ok()?))
    };
    previous
        .into_iter()
        .flat_map(ys)
        .chain(replacement.into_iter().flat_map(ys))
        .filter(|&y| {
            let before = previous.and_then(|column| column.storage(y));
            let after = replacement.and_then(|column| column.storage(y));
            match (before, after) {
                (Some(before), Some(after)) => !Arc::ptr_eq(&before, &after),
                (None, None) => false,
                _ => true,
            }
        })
        .collect()
}

pub(super) fn expand_mesh_dependents(changed: Vec<SubChunkKey>) -> Vec<SubChunkKey> {
    changed
        .into_iter()
        .flat_map(SubChunkKey::mesh_dependents)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(super) fn expand_biome_mesh_dependents(changed: Vec<SubChunkKey>) -> Vec<SubChunkKey> {
    changed
        .into_iter()
        .flat_map(SubChunkKey::biome_mesh_dependents)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
