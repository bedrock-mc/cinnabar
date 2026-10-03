use std::time::Duration;

use protocol::{BlockUpdateEvent, WorldEvent};

use super::*;

/// Admits one prepared event using the same bounded API as the coordinator.
fn queue(state: &mut OrderedCommitState, sequence: u64, event: PreparedWorldEvent, heavy: bool) {
    state.admit(sequence, heavy, 0).unwrap();
    state.insert_ready(sequence, event).unwrap();
}

/// Builds one decoded all-air slot without installed assets or worker execution.
fn air_slot(y: i32) -> PreparedSubChunk {
    PreparedSubChunk {
        position: [0, y, 0],
        result: PreparedSubChunkResult::AllAir,
    }
}

#[test]
fn prepared_completion_waits_for_the_missing_fifo_sequence() {
    let mut state = OrderedCommitState::new(1);
    queue(&mut state, 2, PreparedWorldEvent::CommitOnly, false);
    assert!(state.next_commit().is_none());
    assert_eq!(state.committed_sequence(), 0);
    queue(&mut state, 1, PreparedWorldEvent::CommitOnly, false);
    assert!(matches!(
        state.next_commit(),
        Some(CommitStep::Apply { sequence: 1, .. })
    ));
    assert_eq!(state.committed_sequence(), 1);
    assert!(
        state.next_commit().is_none(),
        "unacknowledged mutation is still active"
    );
    assert!(state.finish_commit(1));
    assert_eq!(state.committed_sequence(), 1);
    assert!(matches!(
        state.next_commit(),
        Some(CommitStep::Apply { sequence: 2, .. })
    ));
    assert!(state.finish_commit(2));
    assert_eq!(state.committed_sequence(), 2);
}

#[test]
fn partial_subchunk_batch_retains_its_frontier_and_admission() {
    let mut state = OrderedCommitState::new(7);
    queue(
        &mut state,
        7,
        PreparedWorldEvent::SubChunks {
            dimension: 0,
            entries: vec![air_slot(0), air_slot(1)],
            duration: Duration::ZERO,
        },
        true,
    );
    queue(&mut state, 8, PreparedWorldEvent::CommitOnly, false);
    assert!(matches!(
        state.next_commit(),
        Some(CommitStep::BatchStarted)
    ));
    assert_eq!(state.committed_sequence(), 6);
    for y in 0..2 {
        let Some(CommitStep::Apply {
            sequence: 7,
            event: PreparedWorldEvent::SubChunks { entries, .. },
        }) = state.next_commit()
        else {
            panic!("one original batch entry must apply next");
        };
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].position[1], y);
        assert_eq!(state.committed_sequence(), 7);
        assert_eq!(state.pending_batch_sequence(), Some(7));
        assert!(state.is_heavy_admitted(7));
        assert_eq!(state.finish_commit(7), y == 1);
        if y == 0 {
            assert_eq!(state.admitted_count(), 2);
            assert_eq!(state.committed_sequence(), 6);
        }
    }
    assert_eq!(state.pending_batch_sequence(), None);
    assert_eq!(state.admitted_count(), 1);
    assert_eq!(state.heavy_count(), 0);
    assert_eq!(state.committed_sequence(), 7);
    assert!(matches!(
        state.next_commit(),
        Some(CommitStep::Apply { sequence: 8, .. })
    ));
}

#[test]
fn async_block_batch_fences_later_events_until_matching_completion() {
    let mut state = OrderedCommitState::new(1);
    queue(
        &mut state,
        1,
        PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
            dimension: 0,
            position: [0, 0, 0],
            layer: 0,
            network_id: 1,
        }])),
        true,
    );
    queue(&mut state, 2, PreparedWorldEvent::CommitOnly, false);
    assert!(matches!(
        state.next_commit(),
        Some(CommitStep::BlockUpdates { sequence: 1, .. })
    ));
    assert_eq!(state.blocking_block_updates(), None);
    state.defer_block_updates(1);
    assert_eq!(state.blocking_block_updates(), Some(1));
    assert!(
        !state.finish_commit(1),
        "only the matching decode may clear its fence"
    );
    assert_eq!(state.committed_sequence(), 0);
    assert!(state.next_commit().is_none());
    state.admit(3, false, 0).unwrap();
    assert!(matches!(
        state
            .complete_decode(3, PreparedWorldEvent::CommitOnly)
            .unwrap(),
        DecodeCommit::Queued
    ));
    assert!(
        state.next_commit().is_none(),
        "unrelated completion must not clear the fence"
    );
    assert!(matches!(
        state
            .complete_decode(
                1,
                PreparedWorldEvent::BlockUpdates {
                    result: Ok(PreparedBlockMutations {
                        mutations: Vec::new(),
                        relight: Default::default()
                    }),
                    duration: Duration::ZERO,
                }
            )
            .unwrap(),
        DecodeCommit::BlockUpdates(_)
    ));
    assert_eq!(state.blocking_block_updates(), None);
    assert_eq!(state.heavy_count(), 0);
    assert_eq!(state.committed_sequence(), 1);
    assert!(matches!(
        state.next_commit(),
        Some(CommitStep::Apply { sequence: 2, .. })
    ));
}

#[test]
fn empty_effective_block_batch_finishes_without_a_worker() {
    let mut state = OrderedCommitState::new(1);
    queue(
        &mut state,
        1,
        PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(Vec::new())),
        true,
    );
    assert!(matches!(
        state.next_commit(),
        Some(CommitStep::BlockUpdates { sequence: 1, .. })
    ));
    assert!(state.finish_commit(1));
    assert_eq!(state.blocking_block_updates(), None);
    assert_eq!(state.admitted_count(), 0);
    assert_eq!(state.committed_sequence(), 1);
    assert!(
        !state.finish_commit(1),
        "finished admission must not be released twice"
    );
}

#[test]
fn admission_counts_retained_consumers_and_independent_heavy_bound() {
    let mut state = OrderedCommitState::new(1);
    assert!(matches!(
        state.admit(1, false, MAX_ADMITTED_WORLD_EVENTS),
        Err(WorldStreamError::AdmissionFull { .. })
    ));
    for sequence in 1..=MAX_ADMITTED_HEAVY_EVENTS as u64 {
        state.admit(sequence, true, 0).unwrap();
    }
    let next = MAX_ADMITTED_HEAVY_EVENTS as u64 + 1;
    assert!(matches!(
        state.admit(next, true, 0),
        Err(WorldStreamError::AdmissionFull { .. })
    ));
    assert!(state.admit(next, false, 0).is_ok());
    assert_eq!(state.remaining_admission_capacity(0), 0);
    state.release_heavy(1);
    assert_eq!(state.remaining_admission_capacity(0), 1);
    assert!(matches!(
        state.admit(1, false, 0),
        Err(WorldStreamError::DuplicateOrPast { .. })
    ));
}

#[test]
fn normalization_releases_heavy_admission_until_its_fifo_turn_finishes() {
    let mut state = OrderedCommitState::new(1);
    state.admit(1, true, 0).unwrap();
    state.release_heavy(1);
    state
        .insert_ready(1, PreparedWorldEvent::NormalizationFailure)
        .unwrap();
    assert_eq!(state.admitted_count(), 1);
    assert_eq!(state.heavy_count(), 0);
    assert!(matches!(
        state.next_commit(),
        Some(CommitStep::Apply { sequence: 1, .. })
    ));
    assert!(state.finish_commit(1));
    assert_eq!(state.admitted_count(), 0);
}

#[test]
fn worker_decode_preserves_air_unavailable_and_sequence_without_assets() {
    use std::{sync::Arc, time::Instant};

    use assets::{NetworkIdMode, RuntimeAssets};
    use protocol::{SubChunkBatchEvent, SubChunkEntryEvent, SubChunkResult, SubChunkUnavailable};

    let assets = Arc::new(RuntimeAssets::diagnostic());
    let biome_tints = Arc::new(assets.biome_assets().resolve_live(&[]).unwrap());
    let job = DecodeJob::SubChunks {
        sequence: 42,
        batch: SubChunkBatchEvent {
            dimension: 0,
            entries: vec![
                SubChunkEntryEvent {
                    position: [2, 3, 4],
                    result: SubChunkResult::AllAir,
                },
                SubChunkEntryEvent {
                    position: [5, 6, 7],
                    result: SubChunkResult::Unavailable(SubChunkUnavailable::YIndexOutOfBounds),
                },
            ],
        },
        ids: DecodeIds {
            assets,
            custom_blocks: 0..0,
            remap: Arc::default(),
            diagnostics: Arc::default(),
            session_id: 1,
            mode: NetworkIdMode::Sequential,
            air: 0,
            biome_tints,
            default_biome: default_biome_id(0),
        },
    };
    let completion = job.run(Instant::now());
    assert_eq!(completion.sequence, 42);
    let PreparedWorldEvent::SubChunks {
        dimension, entries, ..
    } = completion.event
    else {
        panic!("subchunk decode must retain its prepared event kind");
    };
    assert_eq!(dimension, 0);
    assert_eq!(entries[0].position, [2, 3, 4]);
    assert!(matches!(entries[0].result, PreparedSubChunkResult::AllAir));
    assert_eq!(entries[1].position, [5, 6, 7]);
    assert!(matches!(
        entries[1].result,
        PreparedSubChunkResult::Unavailable(SubChunkUnavailable::YIndexOutOfBounds)
    ));
}
