use super::*;

fn stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    })
}

fn submit_publisher(stream: &mut WorldStream, sequence: u64, radius_blocks: u32) {
    stream
        .submit(
            sequence,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0; 3],
                radius_blocks,
            }),
        )
        .expect("admit publisher update");
}

#[test]
fn confirmed_chunk_radius_keeps_fog_independent_of_publisher_shrink() {
    let mut stream = stream();
    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(8))
        .expect("admit confirmed chunk radius");
    submit_publisher(&mut stream, 2, 128);
    assert_eq!(stream.render_distance_blocks(), 128.0);

    submit_publisher(&mut stream, 3, 32);

    assert_eq!(stream.render_distance_blocks(), 128.0);
    assert_eq!(stream.publisher_radius_blocks, Some(32));
    assert_eq!(stream.publisher_radius_chunks, Some(2));
    assert_eq!(stream.active_radius_chunks(), 2);
    assert_eq!(stream.publisher_epoch, 2);
    assert_eq!(stream.committed_view_cohort().unwrap().radius, 2);
}

#[test]
fn confirmed_chunk_radius_changes_and_bounds_fog_distance() {
    let mut stream = stream();
    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(8))
        .expect("admit initial confirmed chunk radius");
    assert_eq!(stream.render_distance_blocks(), 128.0);

    stream
        .submit(2, WorldEvent::ChunkRadiusUpdated(4))
        .expect("admit smaller confirmed chunk radius");
    assert_eq!(stream.render_distance_blocks(), 64.0);

    stream
        .submit(3, WorldEvent::ChunkRadiusUpdated(i32::MAX))
        .expect("admit oversized confirmed chunk radius");
    assert_eq!(
        stream.render_distance_blocks(),
        (super::PHASE0_MAX_VIEW_RADIUS_CHUNKS * 16) as f32
    );

    submit_publisher(&mut stream, 4, 128);
    stream
        .submit(5, WorldEvent::ChunkRadiusUpdated(0))
        .expect("admit zero confirmed chunk radius");
    assert_eq!(stream.render_distance_blocks(), 0.0);
}

#[test]
fn invalid_confirmed_radius_does_not_change_fog_distance() {
    let mut stream = stream();
    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(8))
        .expect("admit valid confirmed chunk radius");
    let invalid_before = stream.stats().normalization_reasons.invalid_chunk_radii;

    stream
        .submit(2, WorldEvent::ChunkRadiusUpdated(-1))
        .expect("sequence semantically invalid chunk radius");

    assert_eq!(stream.render_distance_blocks(), 128.0);
    assert_eq!(
        stream.stats().normalization_reasons.invalid_chunk_radii,
        invalid_before + 1
    );
}

#[test]
fn absent_confirmed_radius_preserves_existing_fallbacks() {
    let mut stream = stream();
    assert_eq!(
        stream.render_distance_blocks(),
        (super::PHASE0_MAX_VIEW_RADIUS_CHUNKS * 16) as f32
    );

    submit_publisher(&mut stream, 1, 32);
    assert_eq!(stream.render_distance_blocks(), 32.0);
}
