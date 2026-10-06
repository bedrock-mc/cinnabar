use super::*;

use super::super::column_set::take_visited_keys;

/// Builds a stream whose extra sections share X with the tested column.
fn stream_with_unrelated_columns(count: i32) -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: RAW_IDS.air,
        block_network_ids_are_hashes: false,
    });
    for z in 10..10 + count {
        for y in [-64, -4, 0, 19, 64] {
            stream.resident.insert(SubChunkKey::new(0, 0, y, z));
            stream.known_air.insert(SubChunkKey::new(0, 0, y, z));
        }
    }
    stream
}

#[test]
fn light_column_queries_visit_only_the_requested_column_at_any_world_height() {
    let mut stream = stream_with_unrelated_columns(0);
    let column = ChunkKey::new(0, 0, 0);
    let expected =
        [i32::MIN, -64, -4, 19, 64, i32::MAX].map(|y| SubChunkKey::from_chunk(column, y));
    stream.resident.extend(expected);
    take_visited_keys();
    assert_eq!(
        stream.light_column_sources(expected[2]).collect::<Vec<_>>(),
        expected
    );
    let before = take_visited_keys();

    stream
        .resident
        .extend(stream_with_unrelated_columns(4096).resident.iter().copied());
    stream.resident.insert(SubChunkKey::new(1, 0, 0, 0));
    stream.resident.insert(SubChunkKey::new(0, 1, 0, 0));
    take_visited_keys();
    assert_eq!(
        stream.light_column_sources(expected[2]).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(take_visited_keys(), before);
    assert_eq!(before, expected.len());
}

#[test]
fn inline_replacement_visits_no_unrelated_resident_or_known_air_sections() {
    let mut visits = Vec::new();
    for unrelated in [0, 4096] {
        let mut stream = stream_with_unrelated_columns(unrelated);
        let column = ChunkKey::new(0, 0, 0);
        let old_solid = SubChunkKey::from_chunk(column, -64);
        let old_air = SubChunkKey::from_chunk(column, 64);
        stream
            .authority
            .commit_sub_chunk(old_solid, uniform_sub_chunk(1))
            .unwrap();
        stream.sync_resident(old_solid);
        stream.record_known_air(old_air);
        stream
            .submit(
                1,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: column.dimension,
                    x: column.x,
                    z: column.z,
                    mode: LevelChunkMode::Inline { count: 0 },
                    payload: biome_payload(0, 1),
                }),
            )
            .unwrap();
        take_visited_keys();
        complete_pending_decode_jobs(&mut stream);
        visits.push(take_visited_keys());

        assert!(!stream.resident.contains(&old_solid));
        assert!(!stream.resident.contains(&old_air));
        assert!(!stream.known_air.contains(&old_air));
        assert!(stream.authority.terrain().sub_chunk(old_solid).is_none());
        let range = vanilla_dimension_range(0).unwrap();
        assert_eq!(
            stream.resident.column(column).count(),
            range.sub_chunk_count as usize
        );
        assert_eq!(
            stream.known_air.column(column).count(),
            range.sub_chunk_count as usize
        );
        for z in 10..10 + unrelated {
            assert!(stream.resident.contains(&SubChunkKey::new(0, 0, 64, z)));
            assert!(stream.known_air.contains(&SubChunkKey::new(0, 0, 64, z)));
        }
    }
    assert_eq!(
        visits[1], visits[0],
        "replacement work grew with unrelated residency"
    );
}

#[test]
fn column_eviction_removes_custom_height_air_without_visiting_other_columns() {
    let mut visits = Vec::new();
    for unrelated in [1, 4096] {
        let mut stream = stream_with_unrelated_columns(unrelated);
        let column = ChunkKey::new(0, 0, 0);
        let keys = [-64, 64].map(|y| SubChunkKey::from_chunk(column, y));
        for key in keys {
            stream.record_known_air(key);
        }
        take_visited_keys();
        stream.evict_column(column);
        visits.push(take_visited_keys());
        assert_eq!(stream.resident.len(), unrelated as usize * 5);
        assert_eq!(stream.known_air.len(), unrelated as usize * 5);
        assert!(keys.iter().all(|key| !stream.resident.contains(key)));
        assert!(keys.iter().all(|key| !stream.known_air.contains(key)));
    }
    assert_eq!(visits[1], visits[0], "eviction visited unrelated residency");
}

/// Retention runs on every local chunk crossing, so finding tracked columns must not walk
/// every resident section.
#[test]
fn tracked_columns_visit_one_section_per_column() {
    let stream = stream_with_unrelated_columns(64);
    take_visited_keys();
    let tracked = stream.tracked_columns();
    assert_eq!(
        tracked,
        (10..74).map(|z| ChunkKey::new(0, 0, z)).collect::<BTreeSet<_>>()
    );
    // One visit per column in each of the resident and known-air sets.
    assert_eq!(take_visited_keys(), 2 * 64);
}

#[test]
fn residency_hash_reuses_unchanged_membership_and_preserves_legacy_key_order() {
    let mut keys = ColumnSubChunkSet::default();
    let first = SubChunkKey::new(0, 0, 19, -2);
    let second = SubChunkKey::new(0, 0, -4, 2);
    let third = SubChunkKey::new(0, 0, 1, -1);
    keys.extend([first, second]);
    take_visited_keys();
    let original = keys.deterministic_hash();
    assert_eq!(take_visited_keys(), 2);
    assert_eq!(
        original,
        deterministic_sub_chunk_key_hash(&BTreeSet::from([first, second]))
    );

    assert!(!keys.insert(first));
    assert!(!keys.remove(&third));
    keys.extend([second, first]);
    take_visited_keys();
    assert_eq!(keys.deterministic_hash(), original);
    assert_eq!(keys.deterministic_hash(), original);
    assert_eq!(
        take_visited_keys(),
        0,
        "unchanged membership rebuilt its witness"
    );

    assert!(keys.insert(third));
    take_visited_keys();
    assert_eq!(
        keys.deterministic_hash(),
        deterministic_sub_chunk_key_hash(&BTreeSet::from([first, second, third]))
    );
    assert_eq!(take_visited_keys(), 3);
    assert!(keys.remove(&third));
    take_visited_keys();
    assert_eq!(keys.deterministic_hash(), original);
    assert_eq!(take_visited_keys(), 2);

    keys.extend([third]);
    take_visited_keys();
    assert_ne!(keys.deterministic_hash(), original);
    assert_eq!(take_visited_keys(), 3);
    keys.clear();
    assert_eq!(
        keys.deterministic_hash(),
        deterministic_sub_chunk_key_hash(&BTreeSet::new())
    );
    let uncached_empty = ColumnSubChunkSet::default();
    assert_eq!(
        keys, uncached_empty,
        "a cached hash is not part of set equality"
    );
}
