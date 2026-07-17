use std::sync::Arc;

use world::{BlockUpdate, ChunkKey, ChunkStore, MAX_STORAGE_COUNT, MutationError, SubChunkKey};

const AIR: u32 = 0;

fn update(layer: u32, x: u8, y: u8, z: u8, runtime_id: u32) -> BlockUpdate {
    BlockUpdate::new(x, y, z, layer, runtime_id)
}

fn coordinates(linear: usize) -> (u8, u8, u8) {
    (
        (linear >> 8) as u8,
        (linear & 15) as u8,
        ((linear >> 4) & 15) as u8,
    )
}

fn zig_zag_i32(value: i32) -> Vec<u8> {
    let mut value = ((value as u32) << 1) ^ ((value >> 31) as u32);
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn uniform(y: i8, runtime_id: u32) -> Vec<u8> {
    let mut bytes = vec![9, 1, y as u8, 1];
    bytes.extend(zig_zag_i32(runtime_id as i32));
    bytes
}

#[test]
fn update_block_creates_sparse_storage_and_removes_it_when_it_returns_to_air() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, -3, -4, 7);

    assert_eq!(
        store
            .update_block(key, update(0, 15, 15, 15, AIR), AIR)
            .unwrap(),
        None,
        "writing air into an absent sub-chunk must not allocate it"
    );
    assert!(store.chunk(key.chunk()).is_none());

    assert_eq!(
        store
            .update_block(key, update(0, 15, 15, 15, 42), AIR)
            .unwrap(),
        Some(key)
    );
    let inserted = store.sub_chunk(key).expect("inserted sub-chunk");
    assert_eq!(inserted.version(), 9);
    assert_eq!(inserted.y_index(), Some(-4));
    assert_eq!(inserted.runtime_id(0, 15, 15, 15), Some(42));
    assert_eq!(inserted.runtime_id(0, 0, 0, 0), Some(AIR));
    assert_eq!(inserted.storages()[0].bits_per_index(), 1);
    assert_eq!(inserted.storages()[0].packed_words().len(), 128);

    assert_eq!(
        store
            .update_block(key, update(0, 15, 15, 15, AIR), AIR)
            .unwrap(),
        Some(key)
    );
    assert!(store.sub_chunk(key).is_none());
    assert!(store.chunk(key.chunk()).is_none());
}

#[test]
fn collision_revision_changes_for_mutation_batches_but_not_final_state_noops() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, -3, -4, 7);
    store.mark_chunk_loaded(key.chunk()).unwrap();
    let loaded = store.collision_revision(key.chunk()).unwrap();

    store
        .update_block(key, update(0, 1, 2, 3, 42), AIR)
        .unwrap();
    let changed = store.collision_revision(key.chunk()).unwrap();
    assert!(changed > loaded);

    store
        .update_sub_chunk_blocks(key, &[update(0, 1, 2, 3, AIR), update(0, 1, 2, 3, 42)], AIR)
        .unwrap();
    assert_eq!(store.collision_revision(key.chunk()), Some(changed));

    store
        .update_sub_chunk_blocks(key, &[update(0, 1, 2, 3, AIR), update(0, 4, 5, 6, 99)], AIR)
        .unwrap();
    assert!(store.collision_revision(key.chunk()).unwrap() > changed);
}

#[test]
fn one_transactional_batch_allocates_one_revision_per_changed_column() {
    let mut store = ChunkStore::new();
    let first = SubChunkKey::new(0, 1, 0, 2);
    let second = SubChunkKey::new(0, 1, 1, 2);
    store.mark_chunk_loaded(first.chunk()).unwrap();
    let before = store.collision_revision(first.chunk()).unwrap();
    let prepared = vec![
        ChunkStore::prepare_sub_chunk_blocks(first, None, &[update(0, 1, 2, 0, 1)], 0).unwrap(),
        ChunkStore::prepare_sub_chunk_blocks(second, None, &[update(0, 3, 4, 0, 2)], 0).unwrap(),
    ];
    store.commit_prepared_block_updates(prepared).unwrap();
    let after = store.collision_revision(first.chunk()).unwrap();
    assert!(after > before);
    let stable = store.collision_revision(first.chunk()).unwrap();
    store.commit_prepared_block_updates(Vec::new()).unwrap();
    assert_eq!(store.collision_revision(first.chunk()), Some(stable));
}

#[test]
fn mutation_grows_from_uniform_width_zero_and_honours_high_bit_air_ids() {
    const HASHED_AIR: u32 = 0xdbf4_4120;
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 1, -4, 2);
    store
        .apply_sub_chunk(key, &uniform(-4, HASHED_AIR))
        .unwrap();
    assert_eq!(
        store.sub_chunk(key).unwrap().storages()[0].bits_per_index(),
        0
    );

    store
        .update_block(key, update(0, 0, 0, 0, 73), HASHED_AIR)
        .unwrap();
    let changed = store.sub_chunk(key).unwrap();
    assert_eq!(changed.storages()[0].bits_per_index(), 1);
    assert_eq!(changed.runtime_id(0, 15, 15, 15), Some(HASHED_AIR));

    store
        .update_block(key, update(0, 0, 0, 0, HASHED_AIR), HASHED_AIR)
        .unwrap();
    assert!(store.sub_chunk(key).is_none());
}

#[test]
fn updates_are_copy_on_write_and_existing_palette_values_are_reused() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 2, 3, 4);
    store
        .update_block(key, update(0, 0, 0, 0, 11), AIR)
        .unwrap();
    let snapshot = store.sub_chunk(key).expect("mesh snapshot");

    assert_eq!(
        store
            .update_block(key, update(0, 1, 0, 0, 11), AIR)
            .unwrap(),
        Some(key)
    );
    let current = store.sub_chunk(key).expect("current sub-chunk");
    assert!(!Arc::ptr_eq(&snapshot, &current));
    assert_eq!(snapshot.runtime_id(0, 1, 0, 0), Some(AIR));
    assert_eq!(current.runtime_id(0, 1, 0, 0), Some(11));
    assert_eq!(current.storages()[0].palette().values(), &[AIR, 11]);
    assert_eq!(current.storages()[0].bits_per_index(), 1);

    let before_noop = Arc::clone(&current);
    assert_eq!(
        store
            .update_block(key, update(0, 1, 0, 0, 11), AIR)
            .unwrap(),
        None
    );
    assert!(Arc::ptr_eq(
        &before_noop,
        &store.sub_chunk(key).expect("unchanged sub-chunk")
    ));
}

#[test]
fn batch_updates_are_atomic_and_create_requested_layers() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(1, 9, -4, -8);

    assert_eq!(
        store
            .update_sub_chunk_blocks(
                key,
                &[
                    update(0, 0, 0, 0, 21),
                    update(1, 15, 15, 15, 31),
                    update(1, 0, 0, 0, 32),
                ],
                AIR,
            )
            .unwrap(),
        vec![key]
    );
    let before_error = store.sub_chunk(key).expect("batched sub-chunk");
    assert_eq!(before_error.storages().len(), 2);
    assert_eq!(before_error.runtime_id(0, 0, 0, 0), Some(21));
    assert_eq!(before_error.runtime_id(1, 15, 15, 15), Some(31));
    assert_eq!(before_error.runtime_id(1, 0, 0, 0), Some(32));

    assert_eq!(
        store.update_sub_chunk_blocks(
            key,
            &[update(0, 0, 0, 0, 99), update(0, 16, 0, 0, 100),],
            AIR,
        ),
        Err(MutationError::LocalCoordinatesOutOfBounds { x: 16, y: 0, z: 0 })
    );
    let after_error = store.sub_chunk(key).expect("unchanged after error");
    assert!(Arc::ptr_eq(&before_error, &after_error));
    assert_eq!(after_error.runtime_id(0, 0, 0, 0), Some(21));

    assert_eq!(
        store.update_sub_chunk_blocks(key, &[update(MAX_STORAGE_COUNT as u32, 0, 0, 0, 101)], AIR,),
        Err(MutationError::LayerOutOfBounds {
            layer: MAX_STORAGE_COUNT as u32,
            max: MAX_STORAGE_COUNT,
        })
    );
    assert!(Arc::ptr_eq(
        &after_error,
        &store.sub_chunk(key).expect("unchanged after layer error")
    ));
}

#[test]
fn batch_final_state_controls_change_reporting_and_air_compaction() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    store.update_block(key, update(0, 0, 0, 0, 7), AIR).unwrap();
    let before = store.sub_chunk(key).unwrap();

    assert!(
        store
            .update_sub_chunk_blocks(key, &[update(0, 0, 0, 0, 8), update(0, 0, 0, 0, 7)], AIR,)
            .unwrap()
            .is_empty()
    );
    assert!(Arc::ptr_eq(&before, &store.sub_chunk(key).unwrap()));

    store
        .update_sub_chunk_blocks(key, &[update(1, 1, 1, 1, 17), update(3, 2, 2, 2, 37)], AIR)
        .unwrap();
    let layered = store.sub_chunk(key).unwrap();
    assert_eq!(layered.storages().len(), 4);
    assert_eq!(layered.runtime_id(2, 15, 15, 15), Some(AIR));

    store
        .update_sub_chunk_blocks(
            key,
            &[update(3, 2, 2, 2, AIR), update(1, 1, 1, 1, AIR)],
            AIR,
        )
        .unwrap();
    let trimmed = store.sub_chunk(key).unwrap();
    assert_eq!(trimmed.storages().len(), 1);
    assert_eq!(trimmed.runtime_id(0, 0, 0, 0), Some(7));
}

#[test]
fn palette_width_growth_uses_bedrock_legal_widths_and_padded_words() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    let checkpoints = [
        (1_usize, 1_u8, 128_usize),
        (2, 2, 256),
        (4, 3, 410),
        (8, 4, 512),
        (16, 5, 683),
        (32, 6, 820),
        (64, 8, 1024),
        (256, 16, 2048),
    ];
    let mut populated = 0;

    for (non_air_values, expected_bits, expected_words) in checkpoints {
        let updates = (populated..non_air_values)
            .map(|linear| {
                let (x, y, z) = coordinates(linear);
                update(0, x, y, z, linear as u32 + 1)
            })
            .collect::<Vec<_>>();
        store.update_sub_chunk_blocks(key, &updates, AIR).unwrap();
        populated = non_air_values;

        let sub_chunk = store.sub_chunk(key).unwrap();
        let storage = &sub_chunk.storages()[0];
        assert_eq!(
            storage.bits_per_index(),
            expected_bits,
            "palette with {} entries",
            non_air_values + 1
        );
        assert_eq!(storage.packed_words().len(), expected_words);
        for linear in [0, non_air_values - 1] {
            let (x, y, z) = coordinates(linear);
            assert_eq!(storage.runtime_id(x, y, z), Some(linear as u32 + 1));
        }

        let (x, y, z) = coordinates(4095);
        store.update_block(key, update(0, x, y, z, 1), AIR).unwrap();
        let sub_chunk = store.sub_chunk(key).unwrap();
        let storage = &sub_chunk.storages()[0];
        assert_eq!(storage.runtime_id(x, y, z), Some(1));
        assert_eq!(storage.bits_per_index(), expected_bits);
        assert_eq!(storage.packed_words().len(), expected_words);
    }
}

#[test]
fn a_full_sixteen_bit_palette_reuses_the_replaced_unique_slot() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    let updates = (0..4096)
        .map(|linear| {
            let (x, y, z) = coordinates(linear);
            update(0, x, y, z, linear as u32 + 1)
        })
        .collect::<Vec<_>>();
    store.update_sub_chunk_blocks(key, &updates, AIR).unwrap();
    let full = store.sub_chunk(key).unwrap();
    assert_eq!(full.storages()[0].palette().len(), 4096);
    assert_eq!(full.storages()[0].bits_per_index(), 16);

    store
        .update_block(key, update(0, 15, 15, 15, 10_000), AIR)
        .unwrap();
    let replaced = store.sub_chunk(key).unwrap();
    assert_eq!(replaced.storages()[0].palette().len(), 4096);
    assert_eq!(replaced.storages()[0].bits_per_index(), 16);
    assert_eq!(replaced.runtime_id(0, 15, 15, 15), Some(10_000));
    assert_eq!(replaced.runtime_id(0, 0, 0, 0), Some(1));
}

#[test]
fn highest_legal_layer_and_local_coordinate_boundaries_are_supported() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(2, i32::MIN, i32::MAX, i32::MAX);
    let highest_layer = MAX_STORAGE_COUNT as u32 - 1;

    assert_eq!(
        store
            .update_block(key, update(highest_layer, 15, 15, 15, 88), AIR)
            .unwrap(),
        Some(key)
    );
    let sub_chunk = store.sub_chunk(key).unwrap();
    assert_eq!(sub_chunk.storages().len(), MAX_STORAGE_COUNT);
    assert_eq!(
        sub_chunk.runtime_id(MAX_STORAGE_COUNT - 1, 15, 15, 15),
        Some(88)
    );

    for invalid in [
        update(0, 16, 15, 15, 1),
        update(0, 15, 16, 15, 1),
        update(0, 15, 15, 16, 1),
    ] {
        assert!(matches!(
            store.update_block(key, invalid, AIR),
            Err(MutationError::LocalCoordinatesOutOfBounds { .. })
        ));
    }
}

#[test]
fn evict_chunk_returns_stored_keys_and_keeps_external_snapshots_alive() {
    let mut store = ChunkStore::new();
    let chunk_key = ChunkKey::new(3, -9, 12);
    let lower = SubChunkKey::from_chunk(chunk_key, -4);
    let upper = SubChunkKey::from_chunk(chunk_key, 19);
    store
        .update_block(upper, update(0, 0, 0, 0, 2), AIR)
        .unwrap();
    store
        .update_block(lower, update(0, 15, 15, 15, 1), AIR)
        .unwrap();
    let lower_snapshot = store.sub_chunk(lower).unwrap();

    assert_eq!(store.evict_chunk(chunk_key), vec![lower, upper]);
    assert!(store.chunk(chunk_key).is_none());
    assert_eq!(lower_snapshot.runtime_id(0, 15, 15, 15), Some(1));
    assert!(store.evict_chunk(chunk_key).is_empty());
}
