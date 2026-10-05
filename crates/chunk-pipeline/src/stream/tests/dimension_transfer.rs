use super::*;

fn destination_stream(dimension: i32) -> WorldStream {
    destination_stream_at(dimension, [0.0; 3])
}

fn destination_stream_at(dimension: i32, position: [f32; 3]) -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: position,
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    })
}

fn admit_air(stream: &mut WorldStream, key: SubChunkKey) {
    stream.authority.mark_sub_chunk_loaded(key).unwrap();
    stream.record_known_air(key);
}

fn air_box(stream: &mut WorldStream, minimum: [i32; 3], maximum: [i32; 3]) {
    let dimension = stream.current_dimension();
    for x in minimum[0]..=maximum[0] {
        for y in minimum[1]..=maximum[1] {
            for z in minimum[2]..=maximum[2] {
                admit_air(stream, SubChunkKey::new(dimension, x, y, z));
            }
        }
    }
}

#[test]
fn all_air_neighbourhood_needs_each_vertical_cell_before_acknowledging() {
    let mut stream = destination_stream(0);
    let missing = SubChunkKey::new(0, 1, 5, 1);
    for x in -1..=1 {
        for y in 3..=5 {
            for z in -1..=1 {
                let key = SubChunkKey::new(0, x, y, z);
                if key != missing {
                    admit_air(&mut stream, key);
                }
            }
        }
    }
    assert!(!stream.dimension_transfer_ready([0.0, 64.0, 0.0]));
    admit_air(&mut stream, missing);
    assert!(stream.dimension_transfer_ready([0.0, 64.0, 0.0]));
    assert!(stream.authority.terrain().chunk(missing.chunk()).is_none());
    assert!(stream.loaded_columns.is_empty());
}

#[test]
fn inclusive_probe_floors_both_corners_before_subchunk_conversion() {
    let mut stream = destination_stream(0);
    air_box(&mut stream, [0, 4, -3], [2, 6, -1]);
    assert!(stream.dimension_transfer_ready([16.25, 80.25, -16.25]));
    assert!(!stream.dimension_transfer_ready([16.25, 96.0, -16.25]));
    assert!(!stream.dimension_transfer_ready([32.0, 80.25, -16.25]));
}

#[test]
fn fractional_y_uses_truncation_for_admission_and_original_value_for_probe() {
    let mut stream = destination_stream(0);
    let minimum = vanilla_dimension_range(0).unwrap().base_sub_chunk_y;
    let minimum_y = (minimum * world::SUB_CHUNK_SIDE as i32) as f32;
    air_box(&mut stream, [-1, minimum, -1], [1, minimum, 1]);
    assert!(stream.dimension_transfer_ready([0.0, minimum_y - 0.1, 0.0]));
    assert!(!stream.dimension_transfer_ready([0.0, minimum_y - 1.1, 0.0]));
    air_box(&mut stream, [-1, -1, -1], [1, 1, 1]);
    assert!(stream.dimension_transfer_ready([0.0, minimum_y - 1.1, 0.0]));
}

#[test]
fn invalid_y_uses_dimension_spawn_instead_of_clamping_to_world_edge() {
    let mut nether = destination_stream(1);
    air_box(&mut nether, [-1, 0, -1], [1, 1, 1]);
    assert!(nether.dimension_transfer_ready([0.0, -1.0, 0.0]));
    assert!(nether.dimension_transfer_ready([0.0, 1000.0, 0.0]));

    let mut end = destination_stream(2);
    air_box(&mut end, [-1, 2, -1], [1, 4, 1]);
    assert!(end.dimension_transfer_ready([0.0, -1.0, 0.0]));
    assert!(end.dimension_transfer_ready([0.0, 1000.0, 0.0]));
}

#[test]
fn unknown_block_ids_are_ready_and_current_dimension_selects_the_terrain() {
    let mut stream = destination_stream(0);
    air_box(&mut stream, [-1, 3, -1], [1, 5, 1]);
    let key = SubChunkKey::new(0, 0, 4, 0);
    stream
        .authority
        .commit_sub_chunk(key, uniform_sub_chunk(u32::MAX))
        .unwrap();
    stream.known_air.remove(&key);
    assert!(stream.dimension_transfer_ready([0.0, 64.0, 0.0]));
    stream.authority.reset_dimension(1, 1);
    assert!(!stream.dimension_transfer_ready([0.0, 64.0, 0.0]));
    air_box(&mut stream, [-1, 3, -1], [1, 5, 1]);
    assert!(stream.dimension_transfer_ready([0.0, 64.0, 0.0]));
}

#[test]
fn custom_dimension_uses_received_destination_cells_without_a_guessed_height() {
    let mut stream = destination_stream(42);
    air_box(&mut stream, [-1, 61, -1], [1, 63, 1]);
    assert!(stream.dimension_transfer_ready([0.0, 1000.0, 0.0]));
    assert!(!stream.dimension_transfer_ready([0.0, 0.0, 0.0]));
    assert!(!stream.dimension_transfer_ready([f32::NAN, 1000.0, 0.0]));
    assert!(!stream.dimension_transfer_ready([0.0, f32::INFINITY, 0.0]));
}

#[test]
fn loading_offsets_cover_the_vanilla_ticking_neighbourhood() {
    use super::super::dimension_transfer::CLIENT_TICKING_OFFSETS;
    let unique = CLIENT_TICKING_OFFSETS
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), CLIENT_TICKING_OFFSETS.len());
    let mut native_shape = std::collections::BTreeSet::new();
    for x in -4_i32..=4 {
        for z in -4_i32..=4 {
            if x.abs() + z.abs() <= 5 {
                native_shape.insert([x, z]);
            }
        }
    }
    assert_eq!(unique, native_shape);
}

#[test]
fn loading_waits_for_selected_columns_even_when_other_columns_are_loaded() {
    use super::super::dimension_transfer::CLIENT_TICKING_OFFSETS;
    let mut stream = destination_stream(1);
    stream.loaded_columns.extend(
        CLIENT_TICKING_OFFSETS
            .iter()
            .map(|offset| ChunkKey::new(1, offset[0], offset[1])),
    );
    assert!(stream.dimension_loading_columns_ready());
    let missing = ChunkKey::new(1, -4, 1);
    stream.loaded_columns.remove(&missing);
    stream.loaded_columns.insert(ChunkKey::new(1, 100, 100));
    assert_eq!(stream.loaded_column_count(), CLIENT_TICKING_OFFSETS.len());
    assert!(!stream.dimension_loading_columns_ready());
    stream.loaded_columns.insert(missing);
    assert!(stream.dimension_loading_columns_ready());
}

#[test]
fn loading_offsets_follow_the_live_dimension_and_negative_chunk_coordinates() {
    use super::super::dimension_transfer::CLIENT_TICKING_OFFSETS;
    let mut stream = destination_stream_at(1, [-0.1, 64.0, -16.1]);
    stream.loaded_columns.extend(
        CLIENT_TICKING_OFFSETS
            .iter()
            .map(|offset| ChunkKey::new(1, offset[0] - 1, offset[1] - 2)),
    );
    assert!(stream.dimension_loading_columns_ready());
    stream.authority.reset_dimension(1, 2);
    assert!(!stream.dimension_loading_columns_ready());
    let wrong_dimension = stream.loaded_columns.clone();
    stream.loaded_columns.extend(
        wrong_dimension
            .iter()
            .map(|key| ChunkKey::new(2, key.x, key.z)),
    );
    assert!(stream.dimension_loading_columns_ready());
}

#[test]
fn dimension_presentation_waits_for_footing_but_not_full_height_neighbor_meshes() {
    let dimension = protocol::NETHER_DIMENSION_ID;
    let position = [
        0.0,
        64.0 + client_world::ingestion::PLAYER_NETWORK_OFFSET,
        0.0,
    ];
    let mut stream = destination_stream_at(dimension, position);
    air_box(&mut stream, [-1, 3, -1], [1, 5, 1]);
    for x in -1..=1 {
        for z in -1..=1 {
            stream.loaded_columns.insert(ChunkKey::new(dimension, x, z));
        }
    }
    // A decoded upper section still awaiting light/mesh publication used to
    // hold the entire loading screen even with collision and footing ready.
    stream.resident.insert(SubChunkKey::new(dimension, 1, 7, 1));
    assert!(!stream.local_terrain_ready());
    assert!(stream.dimension_transfer_presentable(position));

    let footing = SubChunkKey::new(dimension, 0, 3, 0);
    stream.known_air.remove(&footing);
    assert!(stream.dimension_transfer_ready(position));
    assert!(!stream.dimension_transfer_presentable(position));
    admit_air(&mut stream, footing);
    assert!(stream.dimension_transfer_presentable(position));
}
