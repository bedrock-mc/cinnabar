use super::*;

/// Supplies independent world, position and input authority identities.
fn scope() -> InteractionScope {
    InteractionScope {
        session: SessionGeneration::new(7),
        position: PositionAuthorityGeneration::new(3),
        dimension: 0,
        input: InputAuthorityGeneration::INITIAL,
    }
}

#[test]
fn rejected_heads_keep_their_context_and_never_repeat_an_admitted_prefix() {
    let expected = [
        ("attack", 0, [1, 2, 3]),
        ("use", 0, [1, 2, 3]),
        ("select", 1, [1, 2, 3]),
        ("attack", 1, [1, 2, 3]),
    ];
    for refused in 0..expected.len() {
        let mut queue = OrderedInteractions::default();
        queue.synchronize(scope(), false);
        for value in expected {
            queue.push(value, 10).unwrap();
        }
        let mut sent = Vec::new();
        let mut rejected = false;
        while let Some(entry) = queue.pop_front() {
            if sent.len() == refused && !rejected {
                assert_eq!(entry.value, expected[refused]);
                assert_eq!(entry.since_millis, 10);
                queue.restore_front(entry);
                rejected = true;
            } else {
                sent.push(entry.value);
            }
        }
        assert_eq!(sent, expected);
        assert!(rejected);
    }
}

#[test]
fn ui_cancellation_and_expiry_preserve_the_accepted_carrier_fence() {
    let mut queue = OrderedInteractions::default();
    queue.synchronize(scope(), false);
    queue.push("accepted break", 1).unwrap();
    let mut accepted = queue.pop_front().unwrap();
    accepted.admitted = true;
    queue.restore_front(accepted);
    queue.push("pending attack", 2).unwrap();
    assert_eq!(queue.synchronize(scope(), true), Invalidation::Input);
    let accepted = queue.pop_front().unwrap();
    assert_eq!(accepted.value, "accepted break");
    assert!(!accepted.expired(u64::MAX));
    assert!(queue.is_empty());
    queue.push("late press", 20).unwrap();
    let pending = queue.pop_front().unwrap();
    let limit = crate::interaction_authority::MAX_PENDING_INTERACTION_MILLIS;
    assert!(!pending.expired(20 + limit));
    assert!(pending.expired(21 + limit));
}

#[test]
fn world_replacement_retires_old_carrier_markers_and_pending_presses() {
    for replacement in [
        InteractionScope {
            session: SessionGeneration::new(8),
            ..scope()
        },
        InteractionScope {
            position: PositionAuthorityGeneration::new(4),
            ..scope()
        },
        InteractionScope {
            dimension: 1,
            ..scope()
        },
    ] {
        let mut queue = OrderedInteractions::default();
        queue.synchronize(scope(), false);
        queue.push(1, 1).unwrap();
        let mut accepted = queue.pop_front().unwrap();
        accepted.admitted = true;
        queue.restore_front(accepted);
        queue.push(2, 2).unwrap();
        assert_eq!(queue.synchronize(replacement, false), Invalidation::World);
        assert!(queue.is_empty());
    }
}

#[test]
fn burst_overflow_quarantines_input_without_evicting_an_accepted_carrier() {
    let mut queue = OrderedInteractions::default();
    queue.push(0, 0).unwrap();
    let mut accepted = queue.pop_front().unwrap();
    accepted.admitted = true;
    queue.restore_front(accepted);
    for value in 1..MAX_PENDING_ACTION_EVENTS {
        queue.push(value, 0).unwrap();
    }
    assert_eq!(
        queue.push(MAX_PENDING_ACTION_EVENTS, 0),
        Err(MAX_PENDING_ACTION_EVENTS)
    );
    assert!(queue.quarantined());
    let accepted = queue.pop_front().unwrap();
    assert_eq!(accepted.value, 0);
    assert!(accepted.admitted);
    assert!(queue.is_empty());
    queue.observe_neutral(false);
    assert_eq!(queue.push(2, 1), Err(2));
    queue.observe_neutral(true);
    assert_eq!(queue.push(3, 2), Ok(()));
}

#[test]
fn only_a_new_finalized_frame_or_authority_can_append_another_batch() {
    let mut queue = OrderedInteractions::<()>::default();
    let first = InputSequence::new(1);
    assert!(queue.observe_frame(scope(), first));
    assert!(!queue.observe_frame(scope(), first));
    assert!(queue.observe_frame(scope(), first.checked_next().unwrap()));
    let replacement = InteractionScope {
        input: InputAuthorityGeneration::new(std::num::NonZeroU64::new(2).unwrap()),
        ..scope()
    };
    assert!(queue.observe_frame(replacement, first));
}
