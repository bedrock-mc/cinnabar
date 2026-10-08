use world::{
    ChunkKey, ChunkStore, DecodedBiomeColumn, DecodedLevelChunk, DimensionSlots, RawBiomeIds,
    RawBlockIds, SubChunkKey,
};

const IDS: RawBlockIds = RawBlockIds { air: 0 };
const BIOMES: RawBiomeIds = RawBiomeIds { default_biome: 0 };

/// Partial all-air authority stays column-local across completion, eviction and re-entry.
#[test]
fn sparse_collision_authority_retires_only_selected_columns() {
    let mut store = ChunkStore::new();
    let columns = [
        ChunkKey::new(0, 1, 1),
        ChunkKey::new(0, 1, 2),
        ChunkKey::new(1, 1, 1),
    ];
    for column in columns {
        assert!(!store.contains_column(column));
        for y in [-2_000, -4, 20, 2_000] {
            let key = SubChunkKey::from_chunk(column, y);
            assert!(store.mark_sub_chunk_loaded(key).unwrap());
            assert!(!store.mark_sub_chunk_loaded(key).unwrap());
            assert!(store.is_sub_chunk_loaded(key));
        }
        assert!(!store.is_sub_chunk_loaded(SubChunkKey::from_chunk(column, 0)));
        assert!(store.chunk(column).is_none());
        assert!(store.contains_column(column));
    }
    store.mark_chunk_loaded(columns[0]).unwrap();
    assert!(store.contains_column(columns[0]));
    assert!(store.is_sub_chunk_loaded(SubChunkKey::from_chunk(columns[0], 0)));
    assert!(!store.is_sub_chunk_loaded(SubChunkKey::from_chunk(columns[1], 0)));
    let retired_revision = store.collision_revision(columns[1]).unwrap();
    let retained_revision = store.collision_revision(columns[2]);
    let (removed, retired) = store.detach_chunks(&columns[..2].iter().copied().collect());
    assert!(removed.is_empty() && retired.is_empty());
    for column in &columns[..2] {
        assert!(!store.contains_column(*column));
        assert!(!store.is_sub_chunk_loaded(SubChunkKey::from_chunk(*column, -4)));
        assert!(store.collision_revision(*column).is_none());
    }
    assert!(store.is_sub_chunk_loaded(SubChunkKey::from_chunk(columns[2], -4)));
    assert!(store.contains_column(columns[2]));
    assert_eq!(store.collision_revision(columns[2]), retained_revision);
    store
        .mark_sub_chunk_loaded(SubChunkKey::from_chunk(columns[1], -4))
        .unwrap();
    assert!(store.collision_revision(columns[1]).unwrap() > retired_revision);
    assert!(store.contains_column(columns[1]));
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
    store.apply_sub_chunk(key, &first_payload, &IDS).unwrap();
    let first = store
        .collision_revision(chunk)
        .expect("the first retained collision state has an identity");
    assert!(first > loaded);

    store.apply_sub_chunk(key, &first_payload, &IDS).unwrap();
    assert_eq!(store.collision_revision(chunk), Some(first));

    store
        .apply_sub_chunk(key, &uniform(9, Some(-4), 11), &IDS)
        .unwrap();
    let changed = store
        .collision_revision(chunk)
        .expect("the changed collision state has an identity");
    assert!(changed > first);

    store.evict_chunk(chunk);
    assert_eq!(store.collision_revision(chunk), None);
    store.mark_chunk_loaded(chunk).unwrap();
    store.apply_sub_chunk(key, &first_payload, &IDS).unwrap();
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
    store
        .apply_level_chunk(chunk, -4, 1, &payload, &IDS)
        .unwrap();
    let replaced = store
        .collision_revision(chunk)
        .expect("the full-column replacement has an identity");
    assert!(replaced > request_mode);

    store
        .apply_level_chunk(chunk, -4, 1, &payload, &IDS)
        .unwrap();
    assert_eq!(store.collision_revision(chunk), Some(replaced));
}

#[test]
fn individual_sub_chunks_only_dirty_the_store_when_changed() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 4, -4, -7);
    let first = uniform(9, Some(-4), 10);

    assert_eq!(store.apply_sub_chunk(key, &first, &IDS).unwrap(), Some(key));
    assert_eq!(store.apply_sub_chunk(key, &first, &IDS).unwrap(), None);
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 3, 4, 5),
        Some(10)
    );

    let changed = uniform(9, Some(-4), 11);
    assert_eq!(
        store.apply_sub_chunk(key, &changed, &IDS).unwrap(),
        Some(key)
    );
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
        .apply_sub_chunk(key, &uniform(9, Some(-4), 10), &IDS)
        .unwrap();
    let old_snapshot = store.sub_chunk(key).expect("old snapshot");

    store
        .apply_sub_chunk(key, &uniform(9, Some(-4), 11), &IDS)
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
    assert_eq!(
        store.apply_sub_chunk(key, &payload, &IDS).unwrap(),
        Some(key)
    );
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
        .apply_sub_chunk(key, &uniform(9, Some(-4), 12), &IDS)
        .unwrap();

    assert_eq!(store.apply_all_air(key).unwrap(), Some(key));
    assert!(store.sub_chunk(key).is_none());
    assert_eq!(store.apply_all_air(key).unwrap(), None);

    store
        .apply_sub_chunk(key, &uniform(9, Some(-4), 12), &IDS)
        .unwrap();
    let zero_storage = [9, 0, (-4_i8) as u8];
    assert_eq!(
        store.apply_sub_chunk(key, &zero_storage, &IDS).unwrap(),
        Some(key)
    );
    assert!(store.sub_chunk(key).is_none());
}

#[test]
fn biome_only_column_survives_all_air_subchunk_removal() {
    let mut store = ChunkStore::new();
    let chunk = ChunkKey::new(0, 1, 2);
    let key = SubChunkKey::from_chunk(chunk, -4);
    let biomes = DecodedBiomeColumn::decode(-4, 1, &uniform_biome(42), &BIOMES);
    store.commit_biome_column(chunk, biomes);
    store
        .apply_sub_chunk(key, &uniform(9, Some(-4), 12), &IDS)
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
        store
            .apply_sub_chunk(key, &uniform(8, None, 45), &IDS)
            .unwrap(),
        Some(key)
    );
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(45)
    );
}

#[test]
fn individual_payload_is_stored_at_the_requested_key_whatever_its_y_byte() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, -4, 0);
    assert_eq!(
        store.apply_sub_chunk(key, &uniform(9, Some(-3), 1), &IDS),
        Ok(Some(key))
    );
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(1)
    );
}

#[test]
fn level_chunk_reports_block_consumption_and_truncated_reads_empty_their_slots() {
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
        .apply_level_chunk(chunk_key, -4, 2, &payload, &IDS)
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

    let mut truncated = uniform(9, Some(-4), 99);
    truncated.push(9);
    store
        .apply_level_chunk(chunk_key, -4, 2, &truncated, &IDS)
        .unwrap();
    assert_eq!(
        store.sub_chunk(lower_key).unwrap().runtime_id(0, 0, 0, 0),
        Some(99)
    );
    assert!(store.sub_chunk(upper_key).is_none());
}

fn decode_inline(chunk: ChunkKey, slots: usize, count: usize, payload: &[u8]) -> DecodedLevelChunk {
    let slots = DimensionSlots {
        base_sub_chunk_y: -4,
        count: slots,
    };
    DecodedLevelChunk::decode_inline(chunk, slots, count, payload, &IDS, &BIOMES)
}

#[test]
fn inline_level_chunk_decodes_biomes_after_blocks() {
    let mut store = ChunkStore::new();
    let chunk = ChunkKey::new(0, 8, 9);
    let key = SubChunkKey::from_chunk(chunk, -4);
    let block = uniform(9, Some(-4), 20);
    let mut payload = block.clone();
    payload.extend(uniform_biome(7));
    payload.push(0xff);

    let applied = store
        .commit_level_chunk(chunk, decode_inline(chunk, 2, 1, &payload))
        .unwrap();
    assert_eq!(applied.block_bytes_consumed, block.len());
    assert_eq!(applied.bytes_consumed, payload.len());
    assert_eq!(store.biome_id(key, 0, 0, 0), Some(7));
    assert_eq!(
        store.biome_id(SubChunkKey::from_chunk(chunk, -3), 15, 15, 15),
        Some(7)
    );

    // A biome payload cut after its header reads id 0 and extrudes it upwards.
    let mut truncated = uniform(9, Some(-4), 99);
    truncated.push(0x01);
    store
        .commit_level_chunk(chunk, decode_inline(chunk, 2, 1, &truncated))
        .unwrap();
    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(99)
    );
    assert_eq!(store.biome_id(key, 0, 0, 0), Some(0));
    assert_eq!(
        store.biome_id(SubChunkKey::from_chunk(chunk, -3), 0, 0, 0),
        Some(0)
    );
}

#[test]
fn inline_reads_past_the_dimension_slots_consume_nothing_and_wrap_by_low_byte() {
    let chunk = ChunkKey::new(0, 0, 0);
    let blocks = [uniform(9, Some(-4), 1), uniform(9, Some(-3), 2)].concat();
    let mut payload = blocks.clone();
    payload.extend(uniform_biome(7));
    let decoded = decode_inline(chunk, 2, 3, &payload);
    assert_eq!(decoded.block_bytes_consumed(), blocks.len());
    assert_eq!(decoded.sub_chunks().len(), 2);
    let mut store = ChunkStore::new();
    store.commit_level_chunk(chunk, decoded).unwrap();
    assert_eq!(
        store.biome_id(SubChunkKey::from_chunk(chunk, -3), 0, 0, 0),
        Some(7)
    );

    // Reads 256 and 257 land back in slots 0 and 1.
    let wrapped = [
        blocks.as_slice(),
        &uniform(9, Some(-4), 3),
        &uniform(9, Some(-3), 4),
    ]
    .concat();
    let decoded = decode_inline(chunk, 2, 258, &wrapped);
    assert_eq!(decoded.block_bytes_consumed(), wrapped.len());
    let ids = decoded
        .sub_chunks()
        .map(|(y, sub_chunk)| (y, sub_chunk.runtime_id(0, 0, 0, 0)))
        .collect::<Vec<_>>();
    assert_eq!(ids, [(-4, Some(3)), (-3, Some(4))]);
}

#[test]
fn identical_biome_snapshots_reuse_arcs() {
    let mut store = ChunkStore::new();
    let chunk = ChunkKey::new(0, 1, 2);
    let key = SubChunkKey::from_chunk(chunk, -4);
    store.commit_biome_column(
        chunk,
        DecodedBiomeColumn::decode(-4, 1, &uniform_biome(5), &BIOMES),
    );
    let before = store.biome_storage(key).unwrap();

    let dirty = store.commit_biome_column(
        chunk,
        DecodedBiomeColumn::decode(-4, 1, &uniform_biome(5), &BIOMES),
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
    store
        .apply_level_chunk(chunk_key, -4, 2, &initial, &IDS)
        .unwrap();

    let replacement = uniform(9, Some(-4), 20);
    let applied = store
        .apply_level_chunk(chunk_key, -4, 1, &replacement, &IDS)
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
    store
        .apply_level_chunk(chunk_key, -4, 1, &payload, &IDS)
        .unwrap();
    let before = store.sub_chunk(sub_key).unwrap();

    let applied = store
        .apply_level_chunk(chunk_key, -4, 1, &payload, &IDS)
        .unwrap();
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
        .apply_level_chunk(chunk_key, -4, 1, &uniform(9, Some(-4), 20), &IDS)
        .unwrap();

    let zero_storage = [9, 0, (-4_i8) as u8];
    let applied = store
        .apply_level_chunk(chunk_key, -4, 1, &zero_storage, &IDS)
        .unwrap();
    assert_eq!(applied.dirty.len(), 7);
    assert!(applied.dirty.contains(&sub_key));
    assert!(store.chunk(chunk_key).is_none());

    let repeated = store
        .apply_level_chunk(chunk_key, -4, 1, &zero_storage, &IDS)
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
        .apply_level_chunk(chunk_key, -4, 1, &zero_storage, &IDS)
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
fn level_chunk_counts_past_the_payload_read_as_empty_slots() {
    let mut store = ChunkStore::new();
    let key = ChunkKey::new(0, 0, 0);
    let applied = store
        .apply_level_chunk(key, 0, 1_000_000, &[], &IDS)
        .unwrap();
    assert_eq!(applied.bytes_consumed, 0);
    assert!(store.is_chunk_loaded(key));
    assert!(store.chunk(key).is_none());
}

#[test]
fn level_chunk_misplaced_version_nine_sub_chunk_leaves_its_slot_empty() {
    let mut store = ChunkStore::new();
    let key = ChunkKey::new(0, 0, 0);
    let payload = [uniform(9, Some(-4), 1), uniform(9, Some(-2), 2)].concat();
    let applied = store.apply_level_chunk(key, -4, 2, &payload, &IDS).unwrap();
    assert_eq!(applied.bytes_consumed, payload.len());
    assert!(store.sub_chunk(SubChunkKey::from_chunk(key, -4)).is_some());
    assert!(store.sub_chunk(SubChunkKey::from_chunk(key, -3)).is_none());
    assert!(store.sub_chunk(SubChunkKey::from_chunk(key, -2)).is_none());
}
