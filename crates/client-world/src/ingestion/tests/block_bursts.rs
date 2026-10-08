use super::*;

fn update(id: u32) -> PreparedWorldEvent {
    PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
        dimension: 0,
        position: [0, 0, 0],
        layer: 0,
        network_id: id,
    }]))
}

fn completed_updates() -> PreparedWorldEvent {
    PreparedWorldEvent::BlockUpdates {
        result: Ok(PreparedBlockMutations {
            mutations: Vec::new(),
            relight: Default::default(),
        }),
        duration: Duration::ZERO,
    }
}

#[test]
fn consecutive_block_burst_keeps_wire_order_and_one_decode_fence() {
    let mut state = OrderedCommitState::new(1);
    queue(&mut state, 1, update(1), true);
    queue(&mut state, 2, update(2), true);
    queue(&mut state, 3, PreparedWorldEvent::CommitOnly, false);
    let Some(CommitStep::BlockUpdates { sequence, events }) = state.next_commit() else {
        panic!("ready block burst must be prepared together");
    };
    assert_eq!(sequence, 1);
    assert_eq!(state.committed_sequence(), 0);
    assert_eq!(
        events
            .iter()
            .map(|event| event.network_id)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    state.defer_block_updates(sequence);
    assert_eq!(state.committed_sequence(), 0);
    assert_eq!(state.admitted_count(), 3);
    assert_eq!(state.heavy_count(), 2);
    assert!(state.next_commit().is_none());
    assert!(matches!(
        state
            .complete_decode(sequence, completed_updates())
            .unwrap(),
        DecodeCommit::BlockUpdates(_)
    ));
    assert_eq!(state.committed_sequence(), 2);
    assert_eq!(state.admitted_count(), 1);
    assert_eq!(state.heavy_count(), 0);
    assert!(matches!(
        state.next_commit(),
        Some(CommitStep::Apply { sequence: 3, .. })
    ));
}

#[test]
fn block_burst_cannot_cross_a_control_or_missing_sequence() {
    for middle in [Some(PreparedWorldEvent::CommitOnly), None] {
        let mut state = OrderedCommitState::new(1);
        queue(&mut state, 1, update(1), true);
        if let Some(middle) = middle {
            queue(&mut state, 2, middle, false);
        }
        queue(&mut state, 3, update(3), true);
        let Some(CommitStep::BlockUpdates { sequence, events }) = state.next_commit() else {
            panic!("first update must be available");
        };
        assert_eq!(sequence, 1);
        assert_eq!(events.len(), 1);
        state.defer_block_updates(sequence);
        state
            .complete_decode(sequence, completed_updates())
            .unwrap();
        assert_eq!(state.next_sequence(), 2);
        assert_eq!(state.committed_sequence(), 1);
        assert!(state.is_heavy_admitted(3));
    }
}

#[test]
fn empty_coalesced_block_burst_releases_each_admission_without_a_worker() {
    let mut state = OrderedCommitState::new(1);
    for sequence in 1..=MAX_ADMITTED_HEAVY_EVENTS as u64 {
        queue(
            &mut state,
            sequence,
            PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(Vec::new())),
            true,
        );
    }
    let Some(CommitStep::BlockUpdates { sequence, events }) = state.next_commit() else {
        panic!("empty updates must retain their normal FIFO turn");
    };
    assert!(events.is_empty());
    assert!(state.finish_commit(sequence));
    assert_eq!(state.committed_sequence(), MAX_ADMITTED_HEAVY_EVENTS as u64);
    assert_eq!(state.admitted_count(), 0);
    assert_eq!(state.heavy_count(), 0);
    assert!(!state.finish_commit(sequence));
}

#[test]
fn malformed_layer_keeps_its_packet_failure_boundary() {
    let mut state = OrderedCommitState::new(1);
    queue(&mut state, 1, update(1), true);
    let mut invalid = update(2);
    let PreparedWorldEvent::Immediate(WorldEvent::BlockUpdates(events)) = &mut invalid else {
        unreachable!();
    };
    events[0].layer = world::MAX_STORAGE_COUNT;
    queue(&mut state, 2, invalid, true);
    queue(&mut state, 3, update(3), true);
    for expected in 1..=3 {
        let Some(CommitStep::BlockUpdates { sequence, events }) = state.next_commit() else {
            panic!("each packet must retain its isolated commit turn");
        };
        assert_eq!(sequence, expected);
        assert_eq!(events.len(), 1);
        assert!(state.finish_commit(sequence));
    }
    assert_eq!(state.admitted_count(), 0);
}
