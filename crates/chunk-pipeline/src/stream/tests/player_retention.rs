use super::*;

#[test]
fn chunk_grid_retention_follows_player_and_ignores_publisher() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
    assert!(stream.retain_local([325.0, 70.0, 0.5]));
    assert!(!stream.tracked_columns().contains(&near));
    assert!(stream.tracked_columns().contains(&slack_edge));
}

#[test]
fn shrinking_confirmed_radius_evicts_columns_that_leave_the_grid() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
