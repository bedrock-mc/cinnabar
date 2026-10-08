use super::*;

#[test]
fn review_commit_frontier_waits_for_actual_block_mutation_then_releases_fifo_suffix() {
    let mut stream = block_entity_visual_stream();
    stream.submit(1, inline_air_event(0)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    stream.submit(2, worker_block_batch(0, 1)).unwrap();
    stream.commit(3).unwrap();
    stream.apply_ready();
    assert_eq!(stream.order.blocking_block_updates(), Some(2));
    assert_eq!(
        stream.committed_sequence(),
        1,
        "the pending mutation must not advance the commit frontier"
    );
    assert_eq!(stream.inventory_committed_through(), Some(1));
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(stream.inventory_committed_through(), Some(3));
    assert_eq!(stream.committed_sequence(), 3);
}

#[test]
fn inventory_frontier_stops_at_missing_predecessor_then_passes_malformed_chunk() {
    let mut stream = block_entity_visual_stream();
    stream.commit(2).unwrap();
    stream.poll([0.0; 3], 0);
    assert_eq!(stream.inventory_committed_through(), Some(0));
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: vec![0xff],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.take_fatal_error().is_none());
    assert_eq!(stream.inventory_committed_through(), Some(2));
}

#[test]
fn inventory_frontier_refuses_latched_light_failure_without_a_diagnostic() {
    let mut stream = block_entity_visual_stream();
    stream.lighting.fatal_failure = true;
    assert!(stream.take_fatal_error().is_none());
    assert_eq!(stream.inventory_committed_through(), None);
}
