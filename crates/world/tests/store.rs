use world::{
    ChunkKey, ChunkStore, DecodeError, DecodedBiomeColumn, MAX_LEVEL_SUBCHUNKS, SubChunkKey,
};

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

fn uniform(version: u8, y_index: Option<i8>, runtime_id: u32) -> Vec<u8> {
    let mut bytes = vec![version];
    if version >= 8 {
        bytes.push(1);
    }
    if version == 9 {
        bytes.push(y_index.expect("version 9 requires an index") as u8);
    }
    bytes.push(1);
    bytes.extend(zig_zag_i32(runtime_id as i32));
    bytes
}

fn uniform_biome(biome_id: u32) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(zig_zag_i32(biome_id as i32));
    bytes
}

#[test]
fn collision_revision_tracks_real_changes_and_never_reuses_an_evicted_identity() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 4, -4, -7);
    let chunk = key.chunk();
    let first_payload = uniform(9, Some(-4), 10);

    assert_eq!(store.collision_revision(chunk), None);
    store.mark_chunk_loaded(chunk).unwrap();
    let loaded = store
        .collision_revision(chunk)
        .expect("the newly known column has an identity");
    store.apply_sub_chunk(key, &first_payload).unwrap();
    let first = store
        .collision_revision(chunk)
        .expect("the first retained collision state has an identity");
    assert!(first > loaded);

    store.apply_sub_chunk(key, &first_payload).unwrap();
    assert_eq!(store.collision_revision(chunk), Some(first));

    store
        .apply_sub_chunk(key, &uniform(9, Some(-4), 11))
        .unwrap();
    let changed = store
        .collision_revision(chunk)
        .expect("the changed collision state has an identity");
    assert!(changed > first);

    store.evict_chunk(chunk);
    assert_eq!(store.collision_revision(chunk), None);
    store.mark_chunk_loaded(chunk).unwrap();
    store.apply_sub_chunk(key, &first_payload).unwrap();
    let reloaded = store
        .collision_revision(chunk)
        .expect("a reloaded collision state has an identity");
    assert!(reloaded > changed);
}

#[test]
fn collision_revision_marks_request_mode_load_once_and_full_column_noops_are_stable() {
    let mut store = ChunkStore::new();
    let chunk = ChunkKey::new(0, -3, 7);

    store.mark_chunk_loaded(chunk).unwrap();
    let request_mode = store
        .collision_revision(chunk)
        .expect("known request-mode air has an identity");
    store.mark_chunk_loaded(chunk).unwrap();
    assert_eq!(store.collision_revision(chunk), Some(request_mode));

    let payload = uniform(9, Some(-4), 42);
    store.apply_level_chunk(chunk, -4, 1, &payload).unwrap();
    let replaced = store
        .collision_revision(chunk)
        .expect("the full-column replacement has an identity");
    assert!(replaced > request_mode);

    store.apply_level_chunk(chunk, -4, 1, &payload).unwrap();
    assert_eq!(store.collision_revision(chunk), Some(replaced));
}

#[test]
fn individual_sub_chunks_only_dirty_the_store_when_changed() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 4, -4, -7);
    let first = uniform(9, Some(-4), 10);

    assert_eq!(store.apply_sub_chunk(key, &first).unwrap(), Some(key));
    assert_eq!(store.apply_sub_chunk(key, &first).unwrap(), None);
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 3, 4, 5),
        Some(10)
    );

    let changed = uniform(9, Some(-4), 11);
    assert_eq!(store.apply_sub_chunk(key, &changed).unwrap(), Some(key));
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 3, 4, 5),
        Some(11)
    );
}

#[test]
fn mesh_worker_arc_snapshots_survive_replacement() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 4, -4, -7);
    store
        .apply_sub_chunk(key, &uniform(9, Some(-4), 10))
        .unwrap();
    let old_snapshot = store.sub_chunk(key).expect("old snapshot");

    store
        .apply_sub_chunk(key, &uniform(9, Some(-4), 11))
        .unwrap();
    assert_eq!(old_snapshot.runtime_id(0, 0, 0, 0), Some(10));
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(11)
    );
}

#[test]
fn individual_payload_accepts_trailing_block_entity_bytes() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 1, -4, 2);
    let mut payload = uniform(9, Some(-4), 12);
    payload.extend_from_slice(&[0x0a, 0x00, 0x00, 0x00]);
    assert_eq!(store.apply_sub_chunk(key, &payload).unwrap(), Some(key));
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(12)
    );
}

#[test]
fn all_air_responses_remove_stale_data_without_a_flat_storage() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 1, -4, 2);
    store
        .apply_sub_chunk(key, &uniform(9, Some(-4), 12))
        .unwrap();

    assert_eq!(store.apply_all_air(key).unwrap(), Some(key));
    assert!(store.sub_chunk(key).is_none());
    assert_eq!(store.apply_all_air(key).unwrap(), None);

    store
        .apply_sub_chunk(key, &uniform(9, Some(-4), 12))
        .unwrap();
    let zero_storage = [9, 0, (-4_i8) as u8];
    assert_eq!(
        store.apply_sub_chunk(key, &zero_storage).unwrap(),
        Some(key)
    );
    assert!(store.sub_chunk(key).is_none());
}

#[test]
fn biome_only_column_survives_all_air_subchunk_removal() {
    let mut store = ChunkStore::new();
    let chunk = ChunkKey::new(0, 1, 2);
    let key = SubChunkKey::from_chunk(chunk, -4);
    let biomes = DecodedBiomeColumn::decode(-4, 1, &uniform_biome(42)).unwrap();
    store.commit_biome_column(chunk, biomes);
    store
        .apply_sub_chunk(key, &uniform(9, Some(-4), 12))
        .unwrap();

    assert_eq!(store.apply_all_air(key).unwrap(), Some(key));
    assert!(store.sub_chunk(key).is_none());
    assert_eq!(store.biome_id(key, 3, 4, 5), Some(42));
    assert!(store.chunk(chunk).is_some());
}

#[test]
fn external_key_supplies_the_y_index_for_legacy_sub_chunks() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(1, 2, 7, 3);
    assert_eq!(
        store.apply_sub_chunk(key, &uniform(8, None, 45)).unwrap(),
        Some(key)
    );
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(45)
    );
}

#[test]
fn individual_version_nine_index_must_match_the_packet_key() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, -4, 0);
    assert_eq!(
        store.apply_sub_chunk(key, &uniform(9, Some(-3), 1)),
        Err(DecodeError::SubChunkIndexMismatch {
            expected: -4,
            actual: -3,
        })
    );
    assert!(store.sub_chunk(key).is_none());
}

#[test]
fn level_chunk_decode_is_atomic_and_reports_payload_consumption() {
    let mut store = ChunkStore::new();
    let chunk_key = ChunkKey::new(0, 8, 9);
    let lower_key = SubChunkKey::from_chunk(chunk_key, -4);
    let upper_key = SubChunkKey::from_chunk(chunk_key, -3);
    let lower = uniform(9, Some(-4), 20);
    let upper = uniform(9, Some(-3), 30);
    let mut payload = [lower.as_slice(), upper.as_slice()].concat();
    let consumed = payload.len();
    payload.extend_from_slice(&[0xaa, 0xbb, 0xcc]); // Biomes follow block sub-chunks.

    let applied = store
        .apply_level_chunk(chunk_key, -4, 2, &payload)
        .expect("apply full level chunk");
    assert_eq!(applied.bytes_consumed, consumed);
    assert_eq!(applied.dirty.len(), 12);
    assert!(applied.dirty.contains(&lower_key));
    assert!(applied.dirty.contains(&upper_key));
    assert!(
        applied.dirty.contains(&SubChunkKey::new(0, 7, -4, 9)),
        "the horizontal neighbour mesh must be invalidated"
    );
    assert_eq!(
        store.sub_chunk(lower_key).unwrap().runtime_id(0, 0, 0, 0),
        Some(20)
    );
    assert_eq!(
        store.sub_chunk(upper_key).unwrap().runtime_id(0, 0, 0, 0),
        Some(30)
    );

    let mut malformed = uniform(9, Some(-4), 99);
    malformed.push(9); // A truncated second sub-chunk.
    assert!(
        store
            .apply_level_chunk(chunk_key, -4, 2, &malformed)
            .is_err()
    );
    assert_eq!(
        store.sub_chunk(lower_key).unwrap().runtime_id(0, 0, 0, 0),
        Some(20),
        "a failed full decode must not partially replace the chunk"
    );
    assert_eq!(
        store.sub_chunk(upper_key).unwrap().runtime_id(0, 0, 0, 0),
        Some(30)
    );
}

#[test]
fn inline_level_chunk_decodes_biomes_after_blocks_atomically() {
    let mut store = ChunkStore::new();
    let chunk = ChunkKey::new(0, 8, 9);
    let key = SubChunkKey::from_chunk(chunk, -4);
    let block = uniform(9, Some(-4), 20);
    let mut payload = block.clone();
    payload.extend(uniform_biome(7));
    payload.push(0xff);

    let applied = store
        .apply_level_chunk_with_biomes(chunk, -4, 1, -4, 2, &payload)
        .unwrap();
    assert_eq!(applied.block_bytes_consumed, block.len());
    assert_eq!(applied.bytes_consumed, payload.len());
    assert_eq!(store.biome_id(key, 0, 0, 0), Some(7));
    assert_eq!(
        store.biome_id(SubChunkKey::from_chunk(chunk, -3), 15, 15, 15),
        Some(7)
    );

    let mut malformed = uniform(9, Some(-4), 99);
    malformed.extend(uniform_biome(9));
    malformed.push(0xff);
    malformed.push(0xff); // Unexpected third storage is irrelevant; require a bad second instead.
    malformed.truncate(block.len() + 1);
    assert!(
        store
            .apply_level_chunk_with_biomes(chunk, -4, 1, -4, 2, &malformed)
            .is_err()
    );
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(20)
    );
    assert_eq!(store.biome_id(key, 0, 0, 0), Some(7));
}

#[test]
fn identical_biome_snapshots_reuse_arcs() {
    let mut store = ChunkStore::new();
    let chunk = ChunkKey::new(0, 1, 2);
    let key = SubChunkKey::from_chunk(chunk, -4);
    store.commit_biome_column(
        chunk,
        DecodedBiomeColumn::decode(-4, 1, &uniform_biome(5)).unwrap(),
    );
    let before = store.biome_storage(key).unwrap();

    let dirty = store.commit_biome_column(
        chunk,
        DecodedBiomeColumn::decode(-4, 1, &uniform_biome(5)).unwrap(),
    );
    let after = store.biome_storage(key).unwrap();
    assert!(dirty.is_empty());
    assert!(std::sync::Arc::ptr_eq(&before, &after));
}

#[test]
fn a_full_level_chunk_removes_stale_sub_chunks_and_marks_them_dirty() {
    let mut store = ChunkStore::new();
    let chunk_key = ChunkKey::new(0, 1, 2);
    let lower_key = SubChunkKey::from_chunk(chunk_key, -4);
    let upper_key = SubChunkKey::from_chunk(chunk_key, -3);
    let initial = [uniform(9, Some(-4), 20), uniform(9, Some(-3), 30)].concat();
    store.apply_level_chunk(chunk_key, -4, 2, &initial).unwrap();

    let replacement = uniform(9, Some(-4), 20);
    let applied = store
        .apply_level_chunk(chunk_key, -4, 1, &replacement)
        .unwrap();
    assert_eq!(
        applied.changed,
        vec![upper_key],
        "full-column commits must expose unexpanded changed sources"
    );
    assert_eq!(applied.dirty.len(), 7);
    assert!(applied.dirty.contains(&upper_key));
    assert!(
        applied
            .dirty
            .contains(&SubChunkKey::from_chunk(chunk_key, -2)),
        "the vertical neighbour of a removal must be invalidated"
    );
    assert!(store.sub_chunk(lower_key).is_some());
    assert!(store.sub_chunk(upper_key).is_none());
}

#[test]
fn identical_full_level_chunk_reuses_arc_snapshots() {
    let mut store = ChunkStore::new();
    let chunk_key = ChunkKey::new(0, 1, 2);
    let sub_key = SubChunkKey::from_chunk(chunk_key, -4);
    let payload = uniform(9, Some(-4), 20);
    store.apply_level_chunk(chunk_key, -4, 1, &payload).unwrap();
    let before = store.sub_chunk(sub_key).unwrap();

    let applied = store.apply_level_chunk(chunk_key, -4, 1, &payload).unwrap();
    let after = store.sub_chunk(sub_key).unwrap();
    assert!(applied.dirty.is_empty());
    assert!(std::sync::Arc::ptr_eq(&before, &after));
}

#[test]
fn all_air_full_level_chunks_do_not_leave_empty_columns() {
    let mut store = ChunkStore::new();
    let chunk_key = ChunkKey::new(0, 1, 2);
    let sub_key = SubChunkKey::from_chunk(chunk_key, -4);
    store
        .apply_level_chunk(chunk_key, -4, 1, &uniform(9, Some(-4), 20))
        .unwrap();

    let zero_storage = [9, 0, (-4_i8) as u8];
    let applied = store
        .apply_level_chunk(chunk_key, -4, 1, &zero_storage)
        .unwrap();
    assert_eq!(applied.dirty.len(), 7);
    assert!(applied.dirty.contains(&sub_key));
    assert!(store.chunk(chunk_key).is_none());

    let repeated = store
        .apply_level_chunk(chunk_key, -4, 1, &zero_storage)
        .unwrap();
    assert!(repeated.dirty.is_empty());
    assert!(store.chunk(chunk_key).is_none());
}

#[test]
fn level_chunk_residency_survives_sparse_all_air_storage_until_eviction() {
    let mut store = ChunkStore::new();
    let chunk_key = ChunkKey::new(0, -3, 5);
    let zero_storage = [9, 0, (-4_i8) as u8];

    assert!(!store.is_chunk_loaded(chunk_key));
    store
        .apply_level_chunk(chunk_key, -4, 1, &zero_storage)
        .unwrap();
    assert!(
        store.is_chunk_loaded(chunk_key),
        "physics must distinguish a received all-air column from an unknown column"
    );
    assert!(
        store.chunk(chunk_key).is_none(),
        "residency must not allocate a fake flat or empty block column"
    );

    assert!(store.evict_chunk(chunk_key).is_empty());
    assert!(!store.is_chunk_loaded(chunk_key));
}

#[test]
fn request_mode_can_mark_a_sparse_column_loaded_until_normal_eviction() {
    let mut store = ChunkStore::new();
    let chunk_key = ChunkKey::new(0, 7, -9);

    store.mark_chunk_loaded(chunk_key).unwrap();
    assert!(store.is_chunk_loaded(chunk_key));
    assert!(store.chunk(chunk_key).is_none());

    assert!(store.evict_chunk(chunk_key).is_empty());
    assert!(!store.is_chunk_loaded(chunk_key));
}

#[test]
fn mesh_dependents_cover_faces_and_handle_coordinate_edges() {
    let key = SubChunkKey::new(2, 10, -4, -3);
    let dependents = key.mesh_dependents().collect::<Vec<_>>();
    assert_eq!(dependents.len(), 7);
    assert!(dependents.contains(&key));
    assert!(dependents.contains(&SubChunkKey::new(2, 9, -4, -3)));
    assert!(dependents.contains(&SubChunkKey::new(2, 11, -4, -3)));
    assert!(dependents.contains(&SubChunkKey::new(2, 10, -5, -3)));
    assert!(dependents.contains(&SubChunkKey::new(2, 10, -3, -3)));
    assert!(dependents.contains(&SubChunkKey::new(2, 10, -4, -4)));
    assert!(dependents.contains(&SubChunkKey::new(2, 10, -4, -2)));

    let edge = SubChunkKey::new(2, i32::MAX, i32::MIN, i32::MAX);
    assert_eq!(edge.mesh_dependents().count(), 4);
}

#[test]
fn level_chunk_count_and_y_arithmetic_are_bounded() {
    let mut store = ChunkStore::new();
    let key = ChunkKey::new(0, 0, 0);
    assert_eq!(
        store.apply_level_chunk(key, 0, MAX_LEVEL_SUBCHUNKS + 1, &[]),
        Err(DecodeError::TooManySubChunks {
            count: MAX_LEVEL_SUBCHUNKS + 1,
            max: MAX_LEVEL_SUBCHUNKS,
        })
    );

    let payload = [uniform(8, None, 1), uniform(8, None, 2)].concat();
    assert_eq!(
        store.apply_level_chunk(key, i32::MAX, 2, &payload),
        Err(DecodeError::SubChunkYOverflow {
            first: i32::MAX,
            offset: 1,
        })
    );
}

#[test]
fn level_chunk_version_nine_indices_must_match_their_sequence() {
    let mut store = ChunkStore::new();
    let key = ChunkKey::new(0, 0, 0);
    let payload = [uniform(9, Some(-4), 1), uniform(9, Some(-2), 2)].concat();
    assert_eq!(
        store.apply_level_chunk(key, -4, 2, &payload),
        Err(DecodeError::SubChunkIndexMismatch {
            expected: -3,
            actual: -2,
        })
    );
    assert!(store.chunk(key).is_none());
}
