use super::*;

#[test]
fn undrained_ui_commits_apply_bounded_backpressure_without_panicking() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    for sequence in 1..=MAX_ADMITTED_WORLD_EVENTS as u64 {
        stream
            .submit(
                sequence,
                WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 20 })),
            )
            .unwrap();
    }

    assert_eq!(stream.remaining_admission_capacity(), 0);
    assert!(matches!(
        stream.submit(
            MAX_ADMITTED_WORLD_EVENTS as u64 + 1,
            WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 19 })),
        ),
        Err(WorldStreamError::AdmissionFull { .. })
    ));
    assert_eq!(stream.take_committed_ui().len(), MAX_ADMITTED_WORLD_EVENTS);
}

#[test]
fn chunk_grid_retention_follows_player_and_ignores_publisher() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();

    // Two columns loaded within the grid around the origin.
    let near = ChunkKey::new(0, 2, 0);
    let slack_edge = ChunkKey::new(0, 10, 0);
    stream.loaded_columns.insert(near);
    stream.loaded_columns.insert(slack_edge);

    // A publisher update never drives retention: both loaded columns survive it
    // even though its tiny active radius sits thousands of blocks away.
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [2_000, 70, 2_000],
                radius_blocks: 16,
            }),
        )
        .unwrap();
    assert!(stream.tracked_columns().contains(&near));
    assert!(stream.tracked_columns().contains(&slack_edge));

    // Moving the local player recenters the grid on its chunk (20, 0). `near`
    // leaves the grid; `slack_edge` survives at exactly Chebyshev radius + 2.
    stream
        .submit(
            3,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [325.0, 70.0, 0.5],
                ..Default::default()
            }),
        )
        .unwrap();
    assert!(!stream.tracked_columns().contains(&near));
    assert!(stream.tracked_columns().contains(&slack_edge));
}

#[test]
fn shrinking_confirmed_radius_evicts_columns_that_leave_the_grid() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();

    let inner = ChunkKey::new(0, 3, 0);
    let outer = ChunkKey::new(0, 9, 0);
    stream.loaded_columns.insert(inner);
    stream.loaded_columns.insert(outer);

    // A newly confirmed, smaller radius re-evaluates retention around the player.
    // The grid view distance becomes 3, so columns survive to Chebyshev radius +
    // 2 == 4: `inner` stays, `outer` is evicted.
    stream.submit(2, WorldEvent::ChunkRadiusUpdated(2)).unwrap();
    assert!(stream.tracked_columns().contains(&inner));
    assert!(!stream.tracked_columns().contains(&outer));
}
