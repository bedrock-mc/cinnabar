use super::*;

#[test]
fn bootstrap_non_finite_horizontal_position_uses_the_shared_finite_scope_anchor() {
    let bootstrap = WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [f32::NAN, 80.0, f32::INFINITY],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    };
    let expected = super::server_position::resolve_server_position(
        bootstrap.player_position,
        [0.0, super::server_position::SAFE_SERVER_HEIGHT, 0.0],
        None,
    );

    let stream = WorldStream::new(bootstrap);

    assert_eq!(stream.resolved_server_position(), expected);
    let anchor = expected.surface_anchor.unwrap();
    assert!(stream.column_is_active(ChunkKey::new(
        0,
        anchor[0].div_euclid(16),
        anchor[1].div_euclid(16),
    )));
}

#[test]
fn change_dimension_non_finite_horizontal_position_keeps_camera_and_scope_together() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [7.25, 70.0, -8.75],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let change = ChangeDimensionEvent {
        dimension: 1,
        position: [f32::NAN, 32_000.0, f32::INFINITY],
        ..Default::default()
    };
    let expected = super::server_position::resolve_server_position(
        change.position,
        stream.resolved_server_position().position,
        stream.resolved_server_position().surface_anchor,
    );

    stream
        .submit(1, WorldEvent::ChangeDimension(change))
        .unwrap();

    assert_eq!(stream.resolved_server_position(), expected);
    let anchor = expected.surface_anchor.unwrap();
    assert!(stream.column_is_active(ChunkKey::new(
        1,
        anchor[0].div_euclid(16),
        anchor[1].div_euclid(16),
    )));
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [super::CommittedControlEvent::ChangeDimension { resolved, .. }]
            if *resolved == expected
    ));
}

#[test]
fn newer_inline_chunk_is_validated_after_fifo_blocked_publisher_update_commits() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });

    stream.submit(1, inline_air_event(0)).unwrap();
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_600, 64, 0],
                radius_blocks: 256,
            }),
        )
        .unwrap();
    stream.submit(3, inline_air_event(100)).unwrap();

    assert_eq!(stream.pending_decode.len(), 2);
    complete_pending_decode_jobs(&mut stream);

    let key = SubChunkKey::new(0, 100, -4, 0);
    assert_eq!(stream.publisher.center, Some([1_600, 64, 0]));
    assert!(stream.resident.contains(&key) || stream.known_air.contains(&key));
}

#[test]
fn equal_loaded_count_with_missing_target_and_source_replacement_is_not_exact() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let target = super::ViewCohort::from_publisher(0, [160, 64, 160], 16);
    assert_eq!(
        super::ViewCohort {
            dimension: 0,
            center: [0, 0],
            radius: 16,
            publisher_geometry: None,
        }
        .classifier_columns()
        .len(),
        797
    );
    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [160, 64, 160],
                radius_blocks: 16,
            }),
        )
        .unwrap();
    let target_columns = super::ViewCohort {
        publisher_geometry: None,
        ..target
    }
    .classifier_columns();
    stream.publisher.required_columns = target_columns.clone();
    let missing = *target_columns.last().unwrap();
    let source = ChunkKey::new(0, 0, 0);
    stream.loaded_columns.insert(source);
    stream.capture_source_columns();
    stream.loaded_columns = target_columns
        .iter()
        .copied()
        .filter(|column| *column != missing)
        .collect();
    stream.loaded_columns.insert(source);

    let status = stream.cohort_status(target);

    assert_eq!(status.expected, 5);
    assert_eq!(status.loaded_target, 4);
    assert_eq!(status.missing_target, 1);
    assert_eq!(status.foreign_loaded, 1);
    assert_eq!(status.source_leftover, 1);
    assert!(!status.is_exact());

    stream.loaded_columns.remove(&source);
    stream.loaded_columns.insert(missing);

    assert!(stream.cohort_status(target).is_exact());
}

#[test]
fn surface_spawn_waits_for_level_chunk_commit_and_treats_omitted_top_as_air() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 2, -3);
    let block_x = chunk.x * 16 + 5;
    let block_z = chunk.z * 16 + 7;
    assert_eq!(stream.surface_eye_position(block_x, block_z), None);

    let payload = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../world/fixtures/uniform_non_air.bin"
    ));
    let decoded = DecodedLevelChunk::decode(-4, 1, payload, &RAW_IDS);
    stream.apply_prepared(super::PreparedWorldEvent::InlineLevelChunk {
        event: LevelChunkEvent {
            dimension: 0,
            x: chunk.x,
            z: chunk.z,
            mode: LevelChunkMode::Inline { count: 1 },
            payload: payload.to_vec(),
        },
        decoded,
        light_neutral: Vec::new(),
        duration: std::time::Duration::ZERO,
    });

    let expected_network_y = -49.0_f32 + 1.0 + PLAYER_NETWORK_OFFSET;
    assert_eq!(
        stream.surface_eye_position(block_x, block_z),
        Some([
            block_x as f32 + 0.5,
            expected_network_y,
            block_z as f32 + 0.5
        ])
    );

    // The resolved spawn anchor must rest exactly on the surface instead of
    // starting embedded inside it: feet are recovered from a movement anchor
    // as network Y minus the protocol offset, so an eye-height guess left
    // feet 1e-5 blocks inside the surface block on every surface spawn.
    // The standing-player box arithmetic below mirrors `sim::Aabb::player_at`
    // plus `intersects` because client-world cannot depend on `sim` under
    // the architecture dependency policy.
    let feet_y = f64::from(expected_network_y - PLAYER_NETWORK_OFFSET);
    assert_eq!(
        feet_y, -48.0_f64,
        "feet must rest exactly on top of the surface block at y=-49"
    );
    let player_min_y = feet_y;
    let player_max_y = feet_y + 1.8;
    let surface_min_y = -49.0_f64;
    let surface_max_y = -48.0_f64;
    let intersects_surface = player_max_y > surface_min_y && player_min_y < surface_max_y;
    assert!(
        !intersects_surface,
        "the standing-player box must not intersect the surface block"
    );
}
