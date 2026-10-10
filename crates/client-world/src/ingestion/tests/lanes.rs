use protocol::{
    ActorEvent, ActorMoveEvent, ActorPositionOrigin, LevelChunkEvent, LevelChunkMode, UiEvent,
};
use world::ChunkKey;

use super::*;

const CONTEXT: LaneContext = LaneContext {
    local_runtime_id: 1,
    local_unique_id: 1,
    dimension: 0,
};

fn actor_move(runtime_id: u64) -> WorldEvent {
    WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
        dimension: 0,
        runtime_id,
        position: [Some(1.0); 3],
        position_origin: ActorPositionOrigin::NetworkOffset,
        pitch: None,
        yaw: None,
        head_yaw: None,
        on_ground: None,
        teleported: false,
        player_mode: None,
        source_tick: None,
        interpolation: Default::default(),
    }))
}

fn chat() -> WorldEvent {
    let text = std::sync::Arc::<str>::from("hi");
    WorldEvent::Ui(UiEvent::Text(protocol::TextEvent {
        category: protocol::TextCategory::Authored,
        kind: protocol::TextKind::Chat,
        needs_translation: false,
        source: Some(text.clone()),
        message: text.clone(),
        parameters: std::sync::Arc::from([]),
        xuid: text.clone(),
        platform_chat_id: text,
        filtered_message: None,
    }))
}

fn publisher() -> WorldEvent {
    WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
        center: [0, 64, 0],
        radius_blocks: 16,
    })
}

fn level_chunk(x: i32) -> WorldEvent {
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension: 0,
        x,
        z: 0,
        mode: LevelChunkMode::Inline { count: 1 },
        payload: Vec::new(),
    })
}

fn sub_chunks(x: i32) -> WorldEvent {
    WorldEvent::SubChunks(protocol::SubChunkBatchEvent {
        dimension: 0,
        entries: vec![protocol::SubChunkEntryEvent {
            diagnostics: None,
            position: [x, 0, 0],
            result: protocol::SubChunkResult::AllAir,
        }],
    })
}

fn block_update(x: i32) -> WorldEvent {
    WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
        dimension: 0,
        position: [x * 16, 0, 0],
        layer: 0,
        network_id: 1,
    }])
}

/// Admits a classified event; ready ones carry their wire event as the prepared form.
fn admit(state: &mut OrderedCommitState, sequence: u64, event: WorldEvent, ready: bool) {
    state.admit(sequence, classify(&event, CONTEXT), 0).unwrap();
    if ready {
        state
            .insert_ready(sequence, PreparedWorldEvent::Immediate(event))
            .unwrap();
    }
}

fn applied(step: Option<CommitStep>) -> Option<u64> {
    match step? {
        CommitStep::Apply { sequence, .. }
        | CommitStep::BlockUpdates { sequence, .. }
        | CommitStep::SyncedBlockUpdates { sequence, .. } => Some(sequence),
        CommitStep::BatchStarted => None,
    }
}

fn applied_ref(step: &CommitStep) -> u64 {
    match step {
        CommitStep::Apply { sequence, .. }
        | CommitStep::BlockUpdates { sequence, .. }
        | CommitStep::SyncedBlockUpdates { sequence, .. } => *sequence,
        CommitStep::BatchStarted => unreachable!("batches are skipped"),
    }
}

fn commit_one(state: &mut OrderedCommitState) -> Option<u64> {
    let sequence = applied(state.next_commit())?;
    assert!(state.finish_commit(sequence));
    Some(sequence)
}

/// A pass that leaves heavy chunk data to a later one.
const SKIP_CHUNK_DATA: CommitBudget = CommitBudget {
    chunk_data: false,
    ..CommitBudget::UNLIMITED
};

fn commit_within(state: &mut OrderedCommitState, budget: CommitBudget) -> Option<u64> {
    let sequence = applied(state.next_commit_within(budget))?;
    state.finish_commit(sequence);
    Some(sequence)
}

#[test]
fn full_heavy_admission_leaves_light_capacity() {
    let mut state = OrderedCommitState::new(1);
    for sequence in 1..=MAX_ADMITTED_HEAVY_EVENTS as u64 {
        admit(&mut state, sequence, level_chunk(sequence as i32), false);
    }
    assert_eq!(state.heavy_capacity(), 0);
    let next = MAX_ADMITTED_HEAVY_EVENTS as u64 + 1;
    admit(&mut state, next, actor_move(9), true);
    assert_eq!(commit_one(&mut state), Some(next));
    assert_eq!(
        state.committed_sequence(),
        0,
        "chunk decodes still own the frontier"
    );
}

#[test]
fn actor_move_passes_a_pending_block_decode_on_another_column() {
    let mut state = OrderedCommitState::new(1);
    admit(&mut state, 1, block_update(0), true);
    let Some(CommitStep::BlockUpdates { sequence: 1, .. }) = state.next_commit() else {
        panic!("the block batch is first");
    };
    state.defer_block_updates(1);
    admit(&mut state, 2, actor_move(9), true);
    assert_eq!(commit_one(&mut state), Some(2));
    assert_eq!(state.blocking_block_updates(), Some(1));
}

#[test]
fn actor_move_passes_a_pending_level_chunk_decode() {
    let mut state = OrderedCommitState::new(1);
    admit(&mut state, 1, level_chunk(0), false);
    admit(&mut state, 2, actor_move(9), true);
    assert_eq!(commit_one(&mut state), Some(2));
    assert_eq!(state.committed_sequence(), 0);
    assert_eq!(state.committed_past_chunk_data(), 2);
}

#[test]
fn block_update_on_a_loading_column_parks_only_that_column() {
    let mut state = OrderedCommitState::new(1);
    admit(&mut state, 1, level_chunk(0), false);
    admit(&mut state, 2, block_update(0), true);
    admit(&mut state, 3, block_update(1), true);
    assert_eq!(commit_one(&mut state), Some(3));
    assert_eq!(
        state.next_commit().map(|_| ()),
        None,
        "column 0 waits for its chunk"
    );
    state
        .insert_ready(1, PreparedWorldEvent::CommitOnly)
        .unwrap();
    assert_eq!(commit_one(&mut state), Some(1));
    assert_eq!(commit_one(&mut state), Some(2));
    assert_eq!(state.committed_sequence(), 3);
}

#[test]
fn local_authority_waits_for_block_mutations_but_not_chunk_decode() {
    let mut state = OrderedCommitState::new(1);
    admit(&mut state, 1, level_chunk(0), false);
    admit(&mut state, 2, block_update(1), true);
    let Some(CommitStep::BlockUpdates { sequence: 2, .. }) = state.next_commit() else {
        panic!("the block batch on a loaded column is unblocked");
    };
    state.defer_block_updates(2);
    admit(&mut state, 3, WorldEvent::NetworkStackLatency(7), true);
    assert!(state.next_commit().is_none());
    assert!(matches!(
        state.complete_decode(
            2,
            PreparedWorldEvent::BlockUpdates {
                result: Ok(PreparedBlockMutations {
                    mutations: Vec::new(),
                    relight: Default::default(),
                }),
                duration: Duration::ZERO,
            }
        ),
        Ok(DecodeCommit::BlockUpdates(_))
    ));
    assert_eq!(
        commit_one(&mut state),
        Some(3),
        "the chunk decode is still pending"
    );
}

#[test]
fn exhausted_heavy_budget_still_commits_light_events() {
    let mut state = OrderedCommitState::new(1);
    admit(&mut state, 1, level_chunk(0), false);
    state
        .insert_ready(1, PreparedWorldEvent::CommitOnly)
        .unwrap();
    admit(&mut state, 2, actor_move(9), true);
    let light_only = CommitBudget {
        heavy: false,
        chunk_data: true,
        couple_position: true,
    };
    assert_eq!(applied(state.next_commit_within(light_only)), Some(2));
    assert!(!state.last_step_was_heavy());
    assert!(state.finish_commit(2));
    assert!(state.next_commit_within(light_only).is_none());
    assert_eq!(commit_one(&mut state), Some(1));
}

#[test]
fn barriers_and_missing_sequences_hold_everything_later() {
    let mut state = OrderedCommitState::new(1);
    admit(&mut state, 1, level_chunk(0), false);
    admit(
        &mut state,
        2,
        WorldEvent::ChangeDimension(Default::default()),
        true,
    );
    admit(&mut state, 3, actor_move(9), true);
    assert!(state.next_commit().is_none());
    let mut gap = OrderedCommitState::new(1);
    admit(&mut gap, 2, actor_move(9), true);
    assert!(gap.next_commit().is_none(), "sequence 1 is unknown");
}

#[test]
fn chat_and_actor_moves_share_no_consumer() {
    let chat = classify(&chat(), CONTEXT);
    let moved = classify(&actor_move(9), CONTEXT);
    assert!(!chat.consumers.intersects(moved.consumers));
    assert!(!chat.local_authority && !moved.local_authority);
}

/// Splits sequences into per-column and per-consumer histories, the state each one builds.
fn projections(order: &[u64], footprints: &[Footprint]) -> Vec<Vec<u64>> {
    let mut keys = Vec::new();
    for column in 0..3 {
        keys.push(
            order
                .iter()
                .copied()
                .filter(|&sequence| {
                    footprints[sequence as usize - 1]
                        .columns
                        .contains(&ChunkKey::new(0, column, 0))
                })
                .collect::<Vec<_>>(),
        );
    }
    for bit in 0..8 {
        let consumer = Consumers::from_bit(bit);
        keys.push(
            order
                .iter()
                .copied()
                .filter(|&sequence| {
                    footprints[sequence as usize - 1]
                        .consumers
                        .intersects(consumer)
                })
                .collect(),
        );
    }
    keys
}

/// Randomized decode completion order never changes any column's or consumer's history,
/// and local authority never overtakes a block mutation.
#[test]
fn randomized_interleavings_match_strict_wire_order() {
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = move |bound: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % bound
    };
    for _ in 0..500 {
        let count = 1 + next(24) as usize;
        let events = (0..count)
            .map(|_| match next(8) {
                7 => sub_chunks(next(3) as i32),
                0 => level_chunk(next(3) as i32),
                1 => block_update(next(3) as i32),
                2 => actor_move(next(2) + 1),
                3 => WorldEvent::NetworkStackLatency(0),
                4 => chat(),
                5 => publisher(),
                _ => WorldEvent::ActorMotion(protocol::ActorMotionEvent {
                    actor_runtime_id: 1,
                    motion: [0.0; 3],
                    tick: 0,
                }),
            })
            .collect::<Vec<_>>();
        let footprints = events
            .iter()
            .map(|event| classify(event, CONTEXT))
            .collect::<Vec<_>>();
        let mut state = OrderedCommitState::new(1);
        let mut waiting = Vec::new();
        let mut sub_chunk_sequences = Vec::new();
        for (index, event) in events.into_iter().enumerate() {
            let sequence = index as u64 + 1;
            let decoded = matches!(event, WorldEvent::LevelChunk(_) | WorldEvent::SubChunks(_));
            if decoded {
                waiting.push(sequence);
            }
            if matches!(event, WorldEvent::SubChunks(_)) {
                sub_chunk_sequences.push(sequence);
            }
            admit(&mut state, sequence, event, !decoded);
        }
        let mut order = Vec::new();
        let mut fenced = 0..0;
        loop {
            loop {
                // Passes that skip chunk data interleave with full ones without reordering.
                let budget = if next(2) == 0 {
                    SKIP_CHUNK_DATA
                } else {
                    CommitBudget::UNLIMITED
                };
                let Some(step) = state
                    .next_commit_within(budget)
                    .or_else(|| state.next_commit())
                else {
                    break;
                };
                // Each test packet carries one update, so a merged burst spans its length.
                let (sequence, span) = match &step {
                    CommitStep::BlockUpdates { sequence, events } => (*sequence, events.len()),
                    CommitStep::BatchStarted => continue,
                    step => (applied_ref(step), 1),
                };
                let range = sequence..sequence + span as u64;
                if matches!(step, CommitStep::BlockUpdates { .. }) && next(2) == 0 {
                    state.defer_block_updates(sequence);
                    fenced = range;
                } else if state.finish_commit(sequence) {
                    order.extend(range);
                }
            }
            if let Some(sequence) = state.blocking_block_updates()
                && (waiting.is_empty() || next(2) == 0)
            {
                state
                    .complete_decode(
                        sequence,
                        PreparedWorldEvent::BlockUpdates {
                            result: Ok(PreparedBlockMutations {
                                mutations: Vec::new(),
                                relight: Default::default(),
                            }),
                            duration: Duration::ZERO,
                        },
                    )
                    .unwrap();
                order.extend(std::mem::replace(&mut fenced, 0..0));
                continue;
            }
            if waiting.is_empty() {
                break;
            }
            let sequence = waiting.remove(next(waiting.len() as u64) as usize);
            let prepared = if sub_chunk_sequences.contains(&sequence) {
                PreparedWorldEvent::SubChunks {
                    dimension: 0,
                    entries: (0..2).map(air_slot).collect(),
                    duration: Duration::ZERO,
                }
            } else {
                PreparedWorldEvent::CommitOnly
            };
            state.insert_ready(sequence, prepared).unwrap();
        }
        assert_eq!(order.len(), count, "every event commits");
        assert_eq!(state.committed_sequence(), count as u64);
        let strict = (1..=count as u64).collect::<Vec<_>>();
        assert_eq!(
            projections(&order, &footprints),
            projections(&strict, &footprints)
        );
        for (position, &sequence) in order.iter().enumerate() {
            if footprints[sequence as usize - 1].local_authority {
                assert!(order[position..].iter().all(|&later| {
                    later > sequence || !footprints[later as usize - 1].mutation
                }));
            }
        }
    }
}

/// Sustained light traffic behind one stuck decode retains bounded bookkeeping, so each
/// commit's scan stays flat.
#[test]
fn finished_light_traffic_behind_a_stuck_decode_stays_bounded() {
    let mut state = OrderedCommitState::new(1);
    admit(&mut state, 1, level_chunk(0), false);
    for sequence in 2..=10_001 {
        admit(&mut state, sequence, actor_move(9), true);
        assert_eq!(commit_one(&mut state), Some(sequence));
        assert!(state.retained_sequence_records() <= 2);
    }
    assert_eq!(state.committed_sequence(), 0);
    assert_eq!(state.committed_past_chunk_data(), 10_001);
    assert_eq!(state.next_sequence(), 1);
    state
        .insert_ready(1, PreparedWorldEvent::CommitOnly)
        .unwrap();
    assert_eq!(commit_one(&mut state), Some(1));
    assert_eq!(state.committed_sequence(), 10_001);
    assert_eq!(state.retained_sequence_records(), 0);
    assert!(matches!(
        state.validate_sequence(5_000),
        Err(WorldStreamError::DuplicateOrPast { .. })
    ));
}

/// A pass that skips chunk data still commits the chunk a ready block change waits behind,
/// then the change, while unrelated chunk data stays for a full pass.
#[test]
fn skipping_chunk_data_still_commits_what_a_ready_mutation_waits_behind() {
    let mut state = OrderedCommitState::new(1);
    admit(&mut state, 1, level_chunk(0), true);
    admit(&mut state, 2, block_update(0), true);
    admit(&mut state, 3, level_chunk(1), true);
    admit(&mut state, 4, actor_move(9), true);
    let order =
        std::iter::from_fn(|| commit_within(&mut state, SKIP_CHUNK_DATA)).collect::<Vec<_>>();
    assert_eq!(order, [4, 1, 2]);
    assert_eq!(commit_one(&mut state), Some(3));
}

/// A teleport changes retention, so a pass that skips chunk data commits every chunk ahead
/// of it in wire order before the teleport itself.
#[test]
fn skipping_chunk_data_still_commits_what_a_ready_teleport_waits_behind() {
    let mut state = OrderedCommitState::new(1);
    admit(&mut state, 1, level_chunk(0), true);
    admit(&mut state, 2, level_chunk(1), true);
    admit(
        &mut state,
        3,
        WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            mode: protocol::MovePlayerMode::Teleport,
            ..Default::default()
        }),
        true,
    );
    let order =
        std::iter::from_fn(|| commit_within(&mut state, SKIP_CHUNK_DATA)).collect::<Vec<_>>();
    assert_eq!(order, [1, 2, 3]);
}

/// Light terrain cues are not chunk data a pass skips.
#[test]
fn skipping_chunk_data_still_commits_light_terrain_events() {
    let mut state = OrderedCommitState::new(1);
    admit(
        &mut state,
        1,
        WorldEvent::BlockEvent(protocol::BlockEventEvent {
            dimension: 0,
            position: [0, 0, 0],
            event_type: 1,
            event_value: 1,
        }),
        true,
    );
    assert_eq!(commit_within(&mut state, SKIP_CHUNK_DATA), Some(1));
}

/// A partial sub-chunk batch continues in a pass that skips chunk data when a ready block
/// change on its column waits behind it, even with an earlier ready batch that cannot start
/// until this one finishes.
#[test]
fn skipping_chunk_data_still_finishes_a_batch_a_ready_mutation_waits_behind() {
    let two_air_entries = || PreparedWorldEvent::SubChunks {
        dimension: 0,
        entries: (0..2).map(air_slot).collect(),
        duration: Duration::ZERO,
    };
    let mut state = OrderedCommitState::new(1);
    admit(&mut state, 1, sub_chunks(1), false);
    admit(&mut state, 2, sub_chunks(0), false);
    state.insert_ready(2, two_air_entries()).unwrap();
    assert!(matches!(
        state.next_commit(),
        Some(CommitStep::BatchStarted)
    ));
    assert_eq!(
        commit_within(&mut state, CommitBudget::UNLIMITED),
        Some(2),
        "the first entry"
    );
    state.insert_ready(1, two_air_entries()).unwrap();
    admit(&mut state, 3, block_update(0), true);
    let order =
        std::iter::from_fn(|| commit_within(&mut state, SKIP_CHUNK_DATA)).collect::<Vec<_>>();
    assert_eq!(order, [2, 3], "the batch's last entry, then the change");
    assert_eq!(state.committed_sequence(), 0, "the earlier batch waits");
}

/// A mutation behind a queued batch can finish the active batch that holds its slot,
/// regardless of which batch came first on the wire, while respecting the heavy budget.
#[test]
fn skipping_chunk_data_follows_a_queued_batch_dependency() {
    let decoded = |x| PreparedWorldEvent::SubChunks {
        dimension: 0,
        entries: (0..2)
            .map(|y| PreparedSubChunk {
                position: [x, y, 0],
                ..air_slot(y)
            })
            .collect(),
        duration: Duration::ZERO,
    };
    for (active, queued) in [(1, 2), (2, 1)] {
        let mut state = OrderedCommitState::new(1);
        admit(&mut state, active, sub_chunks(0), false);
        admit(&mut state, queued, sub_chunks(1), false);
        state.insert_ready(active, decoded(0)).unwrap();
        assert!(matches!(
            state.next_commit(),
            Some(CommitStep::BatchStarted)
        ));
        assert_eq!(
            commit_within(&mut state, CommitBudget::UNLIMITED),
            Some(active)
        );
        state.insert_ready(queued, decoded(1)).unwrap();
        assert!(state.next_commit_within(SKIP_CHUNK_DATA).is_none());
        admit(&mut state, 3, block_update(1), true);
        admit(&mut state, 4, level_chunk(2), true);
        assert!(
            state
                .next_commit_within(CommitBudget {
                    heavy: false,
                    ..SKIP_CHUNK_DATA
                })
                .is_none()
        );

        let mut order = Vec::new();
        while let Some(step) = state.next_commit_within(SKIP_CHUNK_DATA) {
            if let Some(sequence) = applied(Some(step)) {
                order.push(sequence);
                state.finish_commit(sequence);
            }
        }
        assert_eq!(order, [active, queued, queued, 3]);
        assert_eq!(state.committed_sequence(), 3);
        assert_eq!(
            state.admitted_count(),
            1,
            "unrelated chunk data stays offloaded"
        );
    }
}
