use super::*;

/// Creates a confirmed player grid without a server movement echo.
fn stream() -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, WorldEvent::ChunkRadiusUpdated(2)).unwrap();
    stream
}

/// Populates the exact player grid, retaining overlapping columns from earlier moves.
fn populate_view(stream: &mut WorldStream, center: [i32; 2]) -> usize {
    let radius = stream.chunk_radius.unwrap();
    let reach = radius + world::CHUNK_VIEW_SLACK;
    let mut count = 0;
    for x in center[0] - reach..=center[0] + reach {
        for z in center[1] - reach..=center[1] + reach {
            if chunk_in_view(radius, [x, z], center) {
                let column = ChunkKey::new(stream.current_dimension(), x, z);
                stream.loaded_columns.insert(column);
                stream.record_known_air(SubChunkKey::from_chunk(column, 64));
                count += 1;
            }
        }
    }
    count
}

#[test]
fn ordinary_local_movement_bounds_residency_without_server_echoes() {
    let mut stream = stream();
    let server_position = stream.resolved_server_position();
    let publisher = stream.publisher.center;
    for center in 0..64 {
        let count = populate_view(&mut stream, [center, 0]);
        let position = [center as f32 * 16.0 + 0.5, 70.0, 0.5];
        assert_eq!(stream.retain_local(position), center != 0);
        assert_eq!(stream.loaded_columns.len(), count);
        assert_eq!(stream.resident.len(), count);
        assert!(stream.tracked_columns().iter().all(|key| {
            chunk_in_view(stream.chunk_radius.unwrap(), [key.x, key.z], [center, 0])
        }));
        assert!(stream.column_is_data_interesting(ChunkKey::new(0, center, 0)));
        let generation = stream.connectivity_generation();
        assert!(!stream.retain_local([position[0] + 1.0, 70.0, 0.5]));
        assert_eq!(stream.connectivity_generation(), generation);
    }
    assert_eq!(stream.resolved_server_position(), server_position);
    assert_eq!(stream.publisher.center, publisher);
}

#[test]
fn local_grid_eviction_cancels_out_of_range_requests() {
    let mut stream = stream();
    let old = ChunkKey::new(0, 0, 0);
    stream.enqueue_request(old, 0, 1, None);
    assert!(stream.requests.is_expected(SubChunkKey::from_chunk(old, 0)));
    assert_ne!(stream.pending_request_count(), 0);
    assert!(stream.retain_local([320.0, 70.0, 0.0]));
    assert!(!stream.requests.requested.contains_key(&old));
    assert_eq!(stream.pending_request_count(), 0);
    assert!(stream.pop_next_request().is_none());
}

#[test]
fn local_retention_rejects_stale_owners_and_nonfinite_positions() {
    let mut stream = stream();
    let session = stream.authority.actor_session_id();
    let epoch = stream.form_dimension_epoch();
    for (owner, dimension, observed_epoch, position) in [
        (session.wrapping_add(1), 0, epoch, [320.0, 70.0, 0.0]),
        (session, 1, epoch, [320.0, 70.0, 0.0]),
        (session, 0, epoch.wrapping_add(1), [320.0, 70.0, 0.0]),
        (session, 0, epoch, [f32::NAN, 70.0, 0.0]),
        (session, 0, epoch, [0.0, f32::INFINITY, 0.0]),
    ] {
        assert!(!stream.retain_for_local_player(owner, dimension, observed_epoch, position));
        assert_eq!(stream.local_player_chunk, None);
    }
    assert!(stream.retain_local([-0.5, 70.0, -16.5]));
    assert_eq!(stream.local_player_chunk, Some(ChunkKey::new(0, -1, -2)));
    assert!(!stream.retain_local([-15.5, 70.0, -31.5]));
}

#[test]
fn server_spatial_commits_replace_the_local_retention_center() {
    let mut stream = stream();
    stream.retain_local([320.0, 70.0, 0.0]);
    stream
        .submit(
            2,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [-0.5, 70.0, -16.5],
                mode: MovePlayerMode::Teleport,
                ..Default::default()
            }),
        )
        .unwrap();
    assert_eq!(stream.local_player_chunk, None);
    assert_eq!(stream.last_retention_center, Some(ChunkKey::new(0, -1, -2)));
    stream.take_committed_controls();
    assert!(stream.retain_local([320.0, 70.0, 0.0]));
    let previous_epoch = stream.form_dimension_epoch();
    stream
        .submit(
            3,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 70.0, 0.0],
                ..Default::default()
            }),
        )
        .unwrap();
    assert_eq!(stream.local_player_chunk, None);
    assert!(!stream.retain_for_local_player(
        stream.authority.actor_session_id(),
        0,
        previous_epoch,
        [320.0, 70.0, 0.0],
    ));
}

#[test]
fn deferred_spatial_controls_keep_stale_physics_from_evicting_destination_terrain() {
    let mut stream = stream();
    stream
        .submit(
            2,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [1600.5, 70.0, 0.5],
                mode: MovePlayerMode::Teleport,
                ..Default::default()
            }),
        )
        .unwrap();
    let destination = populate_view(&mut stream, [100, 0]);
    assert!(!stream.retain_local([0.5, 70.0, 0.5]));
    assert_eq!(stream.loaded_columns.len(), destination);
    assert!(stream.column_is_data_interesting(ChunkKey::new(0, 100, 0)));
    stream.take_committed_controls();
    assert!(stream.retain_local([1616.5, 70.0, 0.5]));
}

/// A delayed correction or unmarked move must not prune around its historical anchor before
/// physics reconciles it.
#[test]
fn historical_server_moves_keep_the_current_grid() {
    let correction = WorldEvent::PlayerMovementCorrection(PlayerMovementCorrectionEvent {
        position: [272.5, 70.0, 0.5],
        delta: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        subject: MovementCorrectionSubject::Player,
        on_ground: true,
        tick: 5,
    });
    let unmarked_move = WorldEvent::MovePlayer(MovePlayerEvent {
        runtime_id: 1,
        position: [272.5, 70.0, 0.5],
        source_tick: 5,
        ..Default::default()
    });
    for event in [correction, unmarked_move] {
        for local_grid in [true, false] {
            let mut stream = stream();
            let center = if local_grid {
                assert!(stream.retain_local([320.5, 70.0, 0.5]));
                20
            } else {
                0
            };
            let current = populate_view(&mut stream, [center, 0]);
            stream.submit(2, event.clone()).unwrap();
            assert_eq!(stream.loaded_columns.len(), current);
            let leading = ChunkKey::new(0, center + 2, 0);
            assert!(stream.loaded_columns.contains(&leading));
            stream.take_committed_controls();
            assert!(stream.retain_local([272.5, 70.0, 0.5]));
            assert!(!stream.loaded_columns.contains(&leading));
        }
    }
}

#[test]
fn server_position_retention_replaces_a_stale_local_grid() {
    let mut stream = stream();
    assert!(stream.retain_local([320.5, 70.0, 0.5]));
    stream
        .submit(
            2,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [1600.5, 70.0, 0.5],
                ..Default::default()
            }),
        )
        .unwrap();
    assert_eq!(stream.last_retention_center, Some(ChunkKey::new(0, 20, 0)));
    assert!(stream.retain_for_server_position());
    assert_eq!(stream.local_player_chunk, None);
    assert_eq!(stream.last_retention_center, Some(ChunkKey::new(0, 100, 0)));
    assert!(!stream.retain_for_server_position());
}
