use super::*;
use client_world::ingestion::{Footprint, classify};

/// An expired slice publishes one batch entry; the light control passes it at once.
#[test]
fn sliced_sub_chunk_commit_keeps_its_frontier_while_light_events_pass() {
    let (mut stream, key) = stream_with_one_expected_sub_chunk();
    stream.enqueue_request(key.chunk(), key.y, 3, None);
    stream
        .order
        .admit(2, Footprint::terrain([key.chunk()], true), 0)
        .unwrap();
    stream
        .order
        .insert_ready(
            2,
            PreparedWorldEvent::SubChunks {
                dimension: key.dimension,
                entries: (0..3)
                    .map(|offset| PreparedSubChunk {
                        diagnostics: None,
                        position: [key.x, key.y + offset, key.z],
                        result: PreparedSubChunkResult::AllAir,
                    })
                    .collect(),
                duration: Duration::ZERO,
            },
        )
        .unwrap();
    let set_time = WorldEvent::SetTime(SetTimeEvent { time: 123 });
    stream
        .order
        .admit(3, classify(&set_time, stream.lane_context()), 0)
        .unwrap();
    stream
        .order
        .insert_ready(3, PreparedWorldEvent::Immediate(set_time))
        .unwrap();
    stream.poll_deadline = Some(Instant::now());
    stream.polling = true;
    stream.apply_ready();
    assert!(stream.order.pending_batch_sequence().is_some());
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [CommittedControlEvent::SetTime { sequence: 3, .. }]
    ));
    assert_eq!(stream.committed_sequence(), 1);
    assert_eq!(stream.inventory_committed_through(), Some(3));
    assert!(stream.order.is_heavy_admitted(2));
    stream.apply_ready();
    assert_eq!(stream.committed_sequence(), 1);
    stream.apply_ready();
    assert!(stream.order.pending_batch_sequence().is_none());
    assert!(!stream.order.is_heavy_admitted(2));
    assert_eq!(stream.committed_sequence(), 3);
}

/// Local authority waits for a block mutation's decode regardless of the deadline.
#[test]
fn budget_does_not_bypass_block_mutation_fence() {
    let (mut stream, key) = stream_with_one_expected_sub_chunk();
    let mutation = Footprint {
        mutation: true,
        ..Footprint::terrain([key.chunk()], true)
    };
    stream.order.admit(2, mutation, 0).unwrap();
    stream
        .order
        .insert_ready(
            2,
            PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: key.dimension,
                position: [key.x * 16, key.y * 16, key.z * 16],
                layer: 0,
                network_id: 1,
            }])),
        )
        .unwrap();
    assert!(matches!(
        stream.order.next_commit(),
        Some(CommitStep::BlockUpdates { sequence: 2, .. })
    ));
    stream.order.defer_block_updates(2);
    stream.order.admit(3, Footprint::inventory(), 0).unwrap();
    stream
        .order
        .insert_ready(3, PreparedWorldEvent::CommitOnly)
        .unwrap();
    stream.poll_deadline = Some(Instant::now());
    stream.polling = true;
    stream.apply_ready();
    assert_eq!(stream.order.next_sequence(), 3);
    assert_eq!(stream.order.blocking_block_updates(), Some(2));
    assert_eq!(stream.order.ready_count(), 1);
    assert_eq!(stream.inventory_committed_through(), Some(1));
}

/// Light ingress commits past an exhausted frame allocation without renewing it.
#[test]
fn light_ingress_commits_past_the_spent_poll_deadline() {
    let (mut stream, _) = stream_with_one_expected_sub_chunk();
    let deadline = Instant::now();
    stream.poll_deadline = Some(deadline);
    for sequence in 2..5 {
        stream
            .submit(
                sequence,
                WorldEvent::SetTime(SetTimeEvent {
                    time: sequence as i32,
                }),
            )
            .unwrap();
    }
    assert_eq!(stream.take_committed_controls().len(), 3);
    assert_eq!(stream.poll_deadline, Some(deadline));
    assert_eq!(stream.inventory_committed_through(), Some(4));
}

/// Spent commit time cannot reduce a bounded decode handoff to one job per frame.
#[test]
fn expired_commit_slice_keeps_decode_workers_fed() {
    let (mut stream, _) = stream_with_one_expected_sub_chunk();
    for sequence in 2..6 {
        stream.enqueue_decode_job(DecodeJob::SubChunks {
            sequence,
            batch: SubChunkBatchEvent {
                dimension: 0,
                entries: Vec::new(),
            },
            ids: stream.decode_ids(0),
        });
    }
    stream.poll_deadline = Some(Instant::now());
    stream.dispatch_decode_jobs();
    assert_eq!(stream.in_flight_decode_jobs, 4);
    assert!(stream.pending_decode.is_empty());
    for _ in 0..4 {
        let completion = stream
            .decode_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        stream.accept_decode_completion(completion);
    }
    assert_eq!(stream.in_flight_decode_jobs, 0);
}
