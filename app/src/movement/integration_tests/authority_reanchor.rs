#[test]
fn replay_world_identity_change_falls_back_to_an_authoritative_snap() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(
        Duration::from_millis(100),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(9, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [0.25, 2.620_01, 0.0],
            101,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(2),
        ),
        Ok(PhysicsCorrectionOutcome::Snapped { tick: 102 })
    );
    assert!(physics.is_active());
    assert!(ticker.physics_is_authorized());
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(ticker.take_authority_fault(), None);
}

#[test]
fn replay_request_without_a_retained_tick_falls_back_to_a_current_snap() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(
        Duration::from_millis(100),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(13, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [4.0, 70.620_01, 5.0],
            103,
            false,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(1),
        ),
        Ok(PhysicsCorrectionOutcome::Snapped { tick: 103 })
    );
    assert!(physics.is_active());
    assert!(ticker.physics_is_authorized());
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(ticker.take_authority_fault(), None);
}

#[test]
fn replay_with_a_queued_collision_identity_change_falls_back_to_a_current_snap() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(
        Duration::from_millis(100),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(17, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    ticker
        .outbox
        .back_mut()
        .expect("second completed sample is queued")
        .world_identity = fixture_world_identity(2);

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [0.25, 2.620_01, 0.0],
            101,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(1),
        )
        .unwrap(),
        PhysicsCorrectionOutcome::Snapped { tick: 102 }
    );
    assert!(physics.is_active());
    assert!(ticker.physics_is_authorized());
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(ticker.take_authority_fault(), None);
}

#[test]
fn replay_tick_alignment_mismatch_fails_closed_with_expected_and_actual_ticks() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(
        Duration::from_millis(100),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(23, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    ticker.next_tick = 999;

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [0.25, 2.620_01, 0.0],
            101,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(1),
        ),
        Err(PhysicsAuthorityFault::PendingTickMismatch {
            expected: 103,
            actual: 999,
        })
    );
    assert!(!physics.is_active());
    assert!(!ticker.physics_is_authorized());
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(
        ticker.take_authority_fault().unwrap().fault,
        PhysicsAuthorityFault::PendingTickMismatch {
            expected: 103,
            actual: 999,
        }
    );
}

#[test]
fn stale_explicit_snap_preserves_monotonic_ticker_and_physics_alignment() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(
        Duration::from_millis(50),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(3, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(frame.samples[0].clone())
        .unwrap();

    let outcome = reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [8.0, 71.620_01, 9.0],
        0,
        false,
        PhysicsCorrectionMode::Snap,
        &VersionedFloor(2),
    )
    .unwrap();

    assert_eq!(outcome, PhysicsCorrectionOutcome::Snapped { tick: 101 });
    assert_eq!(physics.state().unwrap().tick, 101);
    assert_eq!(ticker.next_tick(), 102);
    assert_eq!(ticker.pending_count(), 0);
    assert!(ticker.physics_is_authorized());
}

#[test]
fn surface_spawn_reanchor_discards_obsolete_outbox_and_anchors_next_delta() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(completed_sample(101, [1.0, 2.620_01, 0.0]))
        .unwrap();

    ticker.reanchor_surface_spawn(101, [8.0, 71.620_01, 9.0]);
    assert_eq!(
        ticker.pending_count(),
        0,
        "a surface-spawn reanchor must discard obsolete movement"
    );
    assert_eq!(
        flush_player_auth_inputs(
            &mut ticker,
            8,
            Some(evidence_context()),
            |_identity, _packet| Ok::<_, &str>(()),
        ),
        Ok(0),
        "no pre-anchor sample may be admitted later in the frame"
    );

    ticker
        .enqueue_completed_physics(completed_sample(102, [8.5, 71.620_01, 9.0]))
        .unwrap();
    let next = ticker.pop_pending().unwrap().snapshot;
    assert_eq!(next.tick, 102);
    assert_eq!(next.delta, [0.5, 0.0, 0.0]);
}

#[test]
fn completed_sample_overflow_never_drops_oldest_and_records_one_bounded_fault() {
    let mut ticker = MovementTicker::default();
    ticker.reset(11, 0, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    for tick in 1..=OUTBOX_CAPACITY as u64 {
        ticker
            .enqueue_completed_physics(completed_sample(tick, [tick as f32, 0.0, 0.0]))
            .unwrap();
    }
    let oldest = ticker.peek_pending().unwrap().snapshot;

    assert_eq!(
        ticker.enqueue_completed_physics(completed_sample(
            OUTBOX_CAPACITY as u64 + 1,
            [99.0, 0.0, 0.0],
        )),
        Err(PhysicsAuthorityFault::OutboxOverflow)
    );
    assert_eq!(oldest.tick, 1);
    assert_eq!(ticker.dropped_tick_count(), 0);
    assert!(!ticker.physics_is_authorized());
    assert_eq!(ticker.pending_count(), 0);
    let fault = ticker.take_authority_fault().unwrap();
    assert_eq!(fault.session_generation, 11);
    assert_eq!(fault.fault, PhysicsAuthorityFault::OutboxOverflow);
    assert_eq!(fault.pending_count, OUTBOX_CAPACITY);
    assert!(ticker.take_authority_fault().is_none());
}

#[test]
fn explicit_deactivation_does_not_erase_a_pending_authority_fault() {
    let mut ticker = MovementTicker::default();
    ticker.reset(19, 0, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    assert_eq!(
        ticker.enqueue_completed_physics(completed_sample(2, [1.0, 0.0, 0.0])),
        Err(PhysicsAuthorityFault::TickMismatch {
            expected: 1,
            actual: 2,
        })
    );

    ticker.deactivate();

    let fault = ticker.take_authority_fault().unwrap();
    assert_eq!(fault.session_generation, 19);
    assert_eq!(
        fault.fault,
        PhysicsAuthorityFault::TickMismatch {
            expected: 1,
            actual: 2,
        }
    );
}
