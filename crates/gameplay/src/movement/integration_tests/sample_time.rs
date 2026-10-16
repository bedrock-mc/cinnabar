use super::*;

/// Samples through the frame coordinator, then retries across later frames.
#[test]
fn movement_sample_time_survives_backpressure_and_budget_deferral() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::super::LocomotionState::default();
    let mut effects = super::super::LocalMovementEffectTimeline::default();
    let mut speed = super::super::LocalMovementSpeedAuthority::default();
    let sampled_at = Duration::from_secs(1);
    let mut frame = speed_frame(Default::default());
    frame.now = sampled_at;
    frame.delta = Duration::from_millis(100);
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    let mut attempts = Vec::new();
    for _ in 0..2 {
        let result = super::super::flush_player_auth_inputs_guarded(
            &mut ticker,
            1,
            Some(evidence_context()),
            |identity, _, _, original| {
                attempts.push((identity.tick.get(), original));
                Err("full")
            },
        );
        assert_eq!(result, Err(MovementSendError::Transport("full")));
    }
    let later = Duration::from_secs(2);
    let mut frame = speed_frame(Default::default());
    frame.now = later;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    for _ in 0..3 {
        assert_eq!(
            super::super::flush_player_auth_inputs_guarded(
                &mut ticker,
                1,
                Some(evidence_context()),
                |identity, _, _, original| {
                    attempts.push((identity.tick.get(), original));
                    Ok::<_, &str>(())
                },
            ),
            Ok(1),
        );
    }
    assert_eq!(
        attempts,
        vec![
            (101, Some(sampled_at)),
            (101, Some(sampled_at)),
            (101, Some(sampled_at)),
            (102, Some(sampled_at)),
            (103, Some(later)),
        ],
    );
}
