use super::*;
use crate::{BlockUpdate, ChunkStore, RawBlockIds, SubChunk, SubChunkKey};
use std::sync::{Arc, atomic::Ordering};

/// Resets only this fixture lineage's deterministic internal cell-work counters.
fn reset(work: &EditWork) {
    work.scanned_cells.store(0, Ordering::Relaxed);
    work.packed_cells.store(0, Ordering::Relaxed);
    work.materializations.store(0, Ordering::Relaxed);
}

/// Reads internal scans, repacked cells and canonical materializations, excluding snapshot copies.
fn work(work: &EditWork) -> [usize; 3] {
    [
        work.scanned_cells.load(Ordering::Relaxed),
        work.packed_cells.load(Ordering::Relaxed),
        work.materializations.load(Ordering::Relaxed),
    ]
}

/// Converts the public packed cell order into a validated block-update fixture.
fn update(linear: usize, layer: u32, value: u32) -> BlockUpdate {
    BlockUpdate::new(
        (linear >> 8) as u8,
        (linear & 15) as u8,
        ((linear >> 4) & 15) as u8,
        layer,
        value,
    )
}

/// Supplies three individually removable values plus the repeated background value.
fn editable_fixture() -> PalettedStorage {
    let mut storage = packed(
        2,
        vec![0, 11, 12, 13],
        |cell| if cell < 3 { cell + 1 } else { 0 },
    );
    assert!(storage.apply_runtime_update(0, 14));
    storage
}

#[test]
fn production_preparation_defers_compaction_across_repeated_last_use_edits() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut store = ChunkStore::new();
    store
        .update_sub_chunk_blocks(
            key,
            &[update(0, 0, 11), update(1, 0, 12), update(2, 0, 13)],
            0,
        )
        .unwrap();
    store.mark_chunk_loaded(key.chunk()).unwrap();
    let original = store.sub_chunk(key).unwrap();
    let counters = Arc::clone(&original.storages()[0].work);
    let mut oracle = original.storages()[0].clone();
    oracle.work = Default::default();
    reset(&counters);
    for value in 100..228 {
        let previous = store.sub_chunk(key).unwrap();
        let revision = store.collision_revision(key.chunk()).unwrap();
        let prepared =
            ChunkStore::prepare_sub_chunk_blocks(key, Some(&previous), &[update(0, 0, value)], 0)
                .unwrap();
        assert!(prepared.changed());
        assert_eq!(
            prepared.replacement().unwrap().runtime_id(0, 0, 0, 0),
            Some(value)
        );
        assert_eq!(
            store.commit_prepared_block_updates(vec![prepared]).unwrap(),
            [key]
        );
        assert!(store.collision_revision(key.chunk()).unwrap() > revision);
        oracle.apply_runtime_updates(&[(0, value)]);
    }
    assert_eq!(work(&counters), [0, 0, 0]);
    let current = store.sub_chunk(key).unwrap();
    let storage = &current.storages()[0];
    assert_eq!(storage.palette().values(), &[0, 12, 13, 227]);
    assert_eq!(storage.bits_per_index(), 2);
    assert_eq!(original.runtime_id(0, 0, 0, 0), Some(11));
    assert_eq!(storage.words(), oracle.words());
    assert_eq!(work(&counters), [0, BLOCKS_PER_SUB_CHUNK, 1]);
    assert_storage_matches(storage, &oracle);
    assert_eq!(work(&counters), [0, BLOCKS_PER_SUB_CHUNK, 1]);
    let revision = store.collision_revision(key.chunk());
    let noop =
        ChunkStore::prepare_sub_chunk_blocks(key, Some(&current), &[update(0, 0, 227)], 0).unwrap();
    assert!(!noop.changed());
    assert!(
        store
            .commit_prepared_block_updates(vec![noop])
            .unwrap()
            .is_empty()
    );
    assert_eq!(revision, store.collision_revision(key.chunk()));
    assert!(Arc::ptr_eq(&current, &store.sub_chunk(key).unwrap()));
    assert_eq!(work(&counters), [0, BLOCKS_PER_SUB_CHUNK, 1]);
}

#[test]
fn logical_index_readers_do_not_materialize_canonical_words() {
    let storage = editable_fixture();
    reset(&storage.work);
    let mut indices = [0; BLOCKS_PER_SUB_CHUNK];
    storage.palette_indices(&mut indices);
    assert_eq!(&indices[..4], &[3, 1, 2, 0]);
    for x in 0..16 {
        for z in 0..16 {
            let mut column = [0; SUB_CHUNK_SIDE];
            storage.column_palette_indices(x, z, &mut column);
            for (y, &palette_index) in column.iter().enumerate() {
                let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | y;
                assert_eq!(usize::from(palette_index), indices[linear] as usize);
                assert_eq!(
                    storage.runtime_id(x, y as u8, z),
                    Some(storage.palette().values()[indices[linear] as usize])
                );
            }
        }
    }
    assert_eq!(work(&storage.work), [0, 0, 0]);
}

#[test]
fn concurrent_canonical_observation_packs_once_and_clone_edits_stay_isolated() {
    let storage = editable_fixture();
    reset(&storage.work);
    let pointers = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| scope.spawn(|| storage.words().as_ptr() as usize))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(pointers.iter().all(|&pointer| pointer == pointers[0]));
    assert_eq!(work(&storage.work), [0, BLOCKS_PER_SUB_CHUNK, 1]);
    let mut edited = storage.clone();
    let retained = storage.words().to_vec();
    assert_eq!(edited.words().as_ptr(), storage.words().as_ptr());
    edited.apply_runtime_update(1, 15);
    assert_eq!(storage.words(), retained);
    assert_eq!(storage.runtime_id(0, 1, 0), Some(12));
    assert_eq!(edited.runtime_id(0, 1, 0), Some(15));
    assert_eq!(work(&storage.work), [0, BLOCKS_PER_SUB_CHUNK, 1]);
    let mut oracle = storage.clone();
    oracle.work = Default::default();
    oracle.apply_runtime_updates(&[(1, 15)]);
    assert_storage_matches(&edited, &oracle);
    assert_eq!(work(&storage.work), [0, 2 * BLOCKS_PER_SUB_CHUNK, 2]);
}

#[test]
fn repeated_width_shrink_and_regrowth_reuse_slots_without_cell_work() {
    for bits in PACKED_BITS {
        let len = (1
            << SUPPORTED_BITS
                .iter()
                .copied()
                .take_while(|&candidate| candidate < bits)
                .last()
                .unwrap())
            + 1;
        let mut storage = packed(bits, (0..len as u32).collect(), |cell| {
            if cell < len - 1 { cell + 1 } else { 0 }
        });
        // Initialize from the independent canonical batch path without observing future edits.
        storage.apply_runtime_updates(&[(BLOCKS_PER_SUB_CHUNK - 1, len as u32 + 1)]);
        storage.apply_runtime_updates(&[(BLOCKS_PER_SUB_CHUNK - 1, 0)]);
        let mut oracle = storage.clone();
        oracle.work = Default::default();
        reset(&storage.work);
        let words = storage.words.as_ptr();
        for value in 10000..10032 {
            assert!(storage.apply_runtime_update(0, 0));
            assert_eq!(storage.bits_per_index(), bits_for_palette_len(len - 1));
            oracle.apply_runtime_updates(&[(0, 0)]);
            assert!(storage.apply_runtime_update(0, value));
            assert_eq!(storage.bits_per_index(), bits);
            oracle.apply_runtime_updates(&[(0, value)]);
        }
        assert_eq!(storage.words.as_ptr(), words);
        assert_eq!(work(&storage.work), [0, 0, 0]);
        assert_storage_matches(&storage, &oracle);
    }
}

#[test]
fn full_palette_replacements_reuse_bounded_physical_slots() {
    let mut storage = packed(16, (0..BLOCKS_PER_SUB_CHUNK as u32).collect(), |cell| cell);
    let mut oracle = storage.clone();
    oracle.work = Default::default();
    storage.apply_runtime_update(0, BLOCKS_PER_SUB_CHUNK as u32);
    reset(&storage.work);
    let words = storage.words.as_ptr();
    let final_value = 2 * BLOCKS_PER_SUB_CHUNK as u32 + 32;
    for value in BLOCKS_PER_SUB_CHUNK as u32 + 1..=final_value {
        assert!(storage.apply_runtime_update(0, value));
        assert_eq!(
            storage.edits.as_ref().unwrap().slots.len(),
            BLOCKS_PER_SUB_CHUNK
        );
    }
    assert_eq!(storage.words.as_ptr(), words);
    assert_eq!(storage.words.len(), word_count(16));
    assert_eq!(work(&storage.work), [0, 0, 0]);
    oracle.apply_runtime_updates(&[(0, final_value)]);
    assert_storage_matches(&storage, &oracle);
}

/// Appends one signed wire palette integer without changing its unsigned runtime bits.
fn varint(bytes: &mut Vec<u8>, value: u32) {
    let value = value as i32;
    let mut wire = ((value as u32) << 1) ^ ((value >> 31) as u32);
    loop {
        let byte = (wire & 127) as u8;
        wire >>= 7;
        bytes.push(byte | if wire != 0 { 128 } else { 0 });
        if wire == 0 {
            break;
        }
    }
}

/// Decodes a raw fixture so the store proof also sees untouched noncanonical representations.
fn section(storage: PalettedStorage) -> SubChunk {
    let mut bytes = vec![9, 1, 0, storage.bits_per_index << 1 | 1];
    for word in storage.words.iter() {
        bytes.extend(word.to_le_bytes());
    }
    if storage.bits_per_index != 0 {
        varint(&mut bytes, storage.palette.len() as u32);
    }
    for &value in storage.palette.values.iter() {
        varint(&mut bytes, value);
    }
    SubChunk::decode(&bytes, &RawBlockIds { air: 0 })
}

#[test]
fn mutation_result_proof_matches_exact_equality_for_noops_batches_and_layers() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    for previous in [
        section(PalettedStorage::uniform(0)),
        section(PalettedStorage::uniform(u32::MAX)),
        section(packed(2, vec![0, 7, 7, 8], |cell| cell % 3)),
        section(packed(16, vec![0, 7], |cell| cell % 2)),
    ] {
        let original = previous.runtime_id(0, 0, 0, 0).unwrap();
        for updates in [
            vec![],
            vec![update(0, 0, original)],
            vec![update(0, 0, 9)],
            vec![update(0, 0, 9), update(0, 0, original)],
            vec![update(0, 1, 0)],
            vec![update(0, 1, 7)],
            vec![update(0, 0, 0), update(1, 0, 0)],
            vec![update(0, 1, 7), update(0, 1, 0)],
        ] {
            let prepared =
                ChunkStore::prepare_sub_chunk_blocks(key, Some(&previous), &updates, 0).unwrap();
            let exact_changed = prepared
                .replacement()
                .is_none_or(|replacement| replacement != &previous);
            assert_eq!(prepared.changed(), exact_changed, "updates={updates:?}");
        }
    }
    let empty = SubChunk::for_block_updates(key.y);
    let prepared =
        ChunkStore::prepare_sub_chunk_blocks(key, Some(&empty), &[update(0, 0, 0)], 0).unwrap();
    assert!(prepared.changed());
    assert!(prepared.replacement().is_none());
    let prepared = ChunkStore::prepare_sub_chunk_blocks(key, None, &[update(0, 0, 0)], 0).unwrap();
    assert!(!prepared.changed());
}
