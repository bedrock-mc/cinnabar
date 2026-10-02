use super::*;
use protocol::{
    LevelChunkEvent, LevelChunkMode, SubChunkBatchEvent, SubChunkEntryEvent, SubChunkResult,
    WorldEvent,
};

#[test]
fn review_chunks_received_before_move_commit_are_measured() {
    let started = Instant::now();
    let movement = protocol::MovePlayerEvent {
        runtime_id: 1,
        position: [1_040.5, 70.0, 1_040.5],
        ..Default::default()
    };
    let mut tracker = FullViewTeleportTracker::new(true);
    tracker.set_source_mutation_coordinate([0, 58, 0]);
    tracker.begin_world_ready([0.5, 70.0, 0.5], 1);
    assert!(tracker.observe_ingress(&WorldEvent::MovePlayer(movement), 1, started, 0, 10));
    tracker.observe_ingress(
        &WorldEvent::LevelChunk(LevelChunkEvent {
            dimension: 0,
            x: 65,
            z: 65,
            mode: LevelChunkMode::LimitlessRequests,
            payload: vec![],
        }),
        2,
        started + Duration::from_millis(10),
        0,
        10,
    );
    tracker.observe_ingress(
        &WorldEvent::SubChunks(SubChunkBatchEvent {
            dimension: 0,
            entries: vec![SubChunkEntryEvent {
                position: [65, -4, 65],
                result: SubChunkResult::AllAir,
            }],
        }),
        3,
        started + Duration::from_millis(20),
        0,
        10,
    );
    assert!(tracker.commit_move(
        1,
        movement,
        Some(ViewCohort::from_publisher(0, [0, 70, 0], 120))
    ));
    let pending = tracker.pending.as_ref().unwrap();
    assert_eq!(pending.level_chunk_events, 1);
    assert_eq!(pending.sub_chunk_events, 1);
    assert_eq!(
        pending.first_level_chunk_latency,
        Some(Duration::from_millis(10))
    );
    assert_eq!(
        pending.first_sub_chunk_latency,
        Some(Duration::from_millis(20))
    );
}
