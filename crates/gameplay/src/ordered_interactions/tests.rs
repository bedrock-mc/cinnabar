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
fn world_replacement_is_distinct_from_input_cancellation() {
    for replacement in [
        InteractionScope {
            session: SessionGeneration::new(8),
            ..scope()
        },
        InteractionScope {
            dimension: 1,
            ..scope()
        },
    ] {
        let mut frames = OrderedInteractions::default();
        frames.synchronize(scope(), false);
        assert_eq!(frames.synchronize(replacement, false), Invalidation::World);
    }
}

#[test]
fn position_correction_does_not_replace_the_world() {
    let mut frames = OrderedInteractions::default();
    frames.synchronize(scope(), false);
    let corrected = InteractionScope {
        position: PositionAuthorityGeneration::new(4),
        ..scope()
    };
    assert_eq!(frames.synchronize(corrected, false), Invalidation::Position);
    assert_eq!(frames.synchronize(corrected, false), Invalidation::None);
    let replaced = InteractionScope {
        session: SessionGeneration::new(8),
        position: PositionAuthorityGeneration::new(5),
        ..corrected
    };
    assert_eq!(frames.synchronize(replaced, true), Invalidation::World);
}

#[test]
fn incomplete_input_requires_a_complete_neutral_sample() {
    let mut frames = OrderedInteractions::default();
    frames.synchronize(scope(), false);
    frames.quarantine();
    frames.observe_neutral(false);
    assert!(frames.quarantined());
    assert_eq!(frames.synchronize(scope(), true), Invalidation::Input);
    assert!(frames.quarantined());
    frames.observe_neutral(true);
    assert!(!frames.quarantined());
}

#[test]
fn only_a_new_finalized_frame_or_authority_can_claim_another_batch() {
    let mut frames = OrderedInteractions::default();
    let first = InputSequence::new(1);
    assert!(frames.observe_frame(scope(), first));
    assert!(!frames.observe_frame(scope(), first));
    assert!(frames.observe_frame(scope(), first.checked_next().unwrap()));
    let replacement = InteractionScope {
        input: InputAuthorityGeneration::new(std::num::NonZeroU64::new(2).unwrap()),
        ..scope()
    };
    assert!(frames.observe_frame(replacement, first));
}
