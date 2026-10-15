use super::*;
use std::{collections::BTreeMap, sync::Arc};
use world::{BLOCKS_PER_SUB_CHUNK, BlockUpdate, ChunkStore, RawBlockIds, SubChunk, SubChunkKey};

/// Keeps both surface layers uniform after shrinking a previously sixteen-bit edited palette.
fn shrunk_section() -> Arc<SubChunk> {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut store = ChunkStore::new();
    store
        .commit_sub_chunk(
            key,
            SubChunk::decode(&[9, 2, 0, 1, 2, 1, 2], &RawBlockIds { air: 0 }),
        )
        .unwrap();
    let distinct = 257; // Crossing the eight-bit palette capacity requires sixteen-bit words.
    let updates = (0..distinct)
        .flat_map(|cell| {
            (0..SURFACE_LAYERS).map(move |layer| {
                BlockUpdate::new(
                    (cell >> 8) as u8,
                    (cell & 15) as u8,
                    ((cell >> 4) & 15) as u8,
                    layer as u32,
                    cell as u32 + 2,
                )
            })
        })
        .collect::<Vec<_>>();
    store.update_sub_chunk_blocks(key, &updates, 0).unwrap();
    for cell in 0..distinct {
        for layer in 0..SURFACE_LAYERS {
            store
                .update_block(
                    key,
                    BlockUpdate::new(
                        (cell >> 8) as u8,
                        (cell & 15) as u8,
                        ((cell >> 4) & 15) as u8,
                        layer as u32,
                        1,
                    ),
                    0,
                )
                .unwrap();
        }
    }
    store.sub_chunk(key).unwrap()
}

/// Recomputes a slice's reservation from its actual owned storage and section metadata.
fn slice_reservation(slice: &SurfaceSlice) -> usize {
    size_of::<SurfaceSlice>()
        + slice
            .sections
            .iter()
            .map(|section| {
                size_of::<SurfaceSection>()
                    + std::mem::size_of_val(section.storages.as_ref())
                    + section
                        .storages
                        .iter()
                        .map(PalettedStorage::retained_heap_bytes_bound)
                        .sum::<usize>()
                    + section
                        .biome
                        .as_ref()
                        .map_or(0, BiomeStorage::retained_heap_bytes_bound)
            })
            .sum::<usize>()
}

#[test]
fn slices_charge_shrunk_storage_and_progress_without_materializing_words() {
    let section = shrunk_section();
    let retained = section
        .storages()
        .iter()
        .map(PalettedStorage::retained_heap_bytes)
        .sum::<usize>();
    assert!(
        section
            .storages()
            .iter()
            .all(|storage| storage.bits_per_index() == 0)
    );
    assert!(retained > SURFACE_LAYERS * BLOCKS_PER_SUB_CHUNK);
    let sections = 32;
    let source = ColumnSnapshot::new(
        Arc::new((0..sections).map(|y| (y, Arc::clone(&section))).collect()),
        Vec::new(),
    );
    assert!(source.retained_payload_bytes() >= sections as usize * retained);
    let mut before = None;
    let mut copied = 0;
    loop {
        let slice = source.surface_slice(before, SurfaceSlice::MIN_BYTES);
        assert!(
            !slice.sections.is_empty(),
            "the minimum must allow progress"
        );
        assert!(
            slice.sections.len() < sections as usize,
            "physical storage must limit admission"
        );
        assert_eq!(slice.retained_bytes(), slice_reservation(&slice));
        assert!(slice.retained_bytes() <= SurfaceSlice::MIN_BYTES);
        for copied_section in &slice.sections {
            assert_eq!(
                copied_section
                    .storages
                    .iter()
                    .map(PalettedStorage::retained_heap_bytes)
                    .sum::<usize>(),
                retained
            );
        }
        copied += slice.sections.len();
        assert!(copied <= sections as usize);
        before = slice.next();
        if before.is_none() {
            break;
        }
    }
    assert_eq!(copied, sections as usize);
    assert_eq!(
        section
            .storages()
            .iter()
            .map(PalettedStorage::retained_heap_bytes)
            .sum::<usize>(),
        retained,
        "accounting and copying must not materialize canonical words"
    );
}

#[test]
fn materialized_cache_is_charged_in_full_when_a_slice_shares_it() {
    let section = shrunk_section();
    for storage in section.storages() {
        assert!(storage.words().is_empty());
    }
    let retained = section
        .storages()
        .iter()
        .map(PalettedStorage::retained_heap_bytes)
        .sum::<usize>();
    let source = ColumnSnapshot::new(Arc::new(BTreeMap::from([(0, section)])), Vec::new());
    let slice = source.surface_slice(None, SurfaceSlice::MIN_BYTES);
    assert_eq!(slice.sections.len(), 1);
    assert_eq!(slice.retained_bytes(), slice_reservation(&slice));
    assert_eq!(
        slice.retained_bytes(),
        size_of::<SurfaceSlice>()
            + size_of::<SurfaceSection>()
            + SURFACE_LAYERS * size_of::<PalettedStorage>()
            + retained
    );
}

#[test]
fn minimum_slice_progresses_with_maximum_edited_layers_and_materialized_caches() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut store = ChunkStore::new();
    let updates = (0..BLOCKS_PER_SUB_CHUNK)
        .flat_map(|cell| {
            (0..SURFACE_LAYERS).map(move |layer| {
                BlockUpdate::new(
                    (cell >> 8) as u8,
                    (cell & 15) as u8,
                    ((cell >> 4) & 15) as u8,
                    layer as u32,
                    cell as u32 + 1,
                )
            })
        })
        .collect::<Vec<_>>();
    store.update_sub_chunk_blocks(key, &updates, 0).unwrap();
    for layer in 0..SURFACE_LAYERS {
        store
            .update_block(
                key,
                BlockUpdate::new(0, 0, 0, layer as u32, BLOCKS_PER_SUB_CHUNK as u32 + 1),
                0,
            )
            .unwrap();
    }
    let section = store.sub_chunk(key).unwrap();
    for storage in section.storages() {
        let _ = storage.words();
        assert_eq!(
            storage.retained_heap_bytes(),
            PalettedStorage::MAX_RETAINED_HEAP_BYTES
        );
    }
    let source = ColumnSnapshot::new(Arc::new(BTreeMap::from([(0, section)])), Vec::new());
    let slice = source.surface_slice(None, SurfaceSlice::MIN_BYTES);
    assert_eq!(slice.sections.len(), 1);
    assert!(slice.next().is_none());
    assert_eq!(slice.retained_bytes(), slice_reservation(&slice));
    assert!(slice.retained_bytes() <= SurfaceSlice::MIN_BYTES);
}
