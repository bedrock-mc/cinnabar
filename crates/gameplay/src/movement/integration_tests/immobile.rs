#[test]
fn immobile_frame_holds_an_airborne_anchor_and_keeps_input_and_look() {
    let mut physics = LocalPhysicsController::default();
    let position = [0.0, 50.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    physics.reanchor_network_position(position, 100, false);
    physics.queue_server_motion([0.5, -1.0, -0.4], 0);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, position);
    ticker.set_source(MovementSource::Physics);
    let mut locals = super::LocomotionState::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    for index in 0..4 {
        let mut frame = speed_frame(Default::default());
        frame.now += Duration::from_millis(index * 50);
        frame.facts.immobile = true;
        frame.pitch = index as f32 * 5.0;
        frame.yaw = index as f32 * 10.0;
        frame.raw_movement = [0.25, 1.0];
        frame.analogue_movement = [0.25, 1.0];
        frame.jump.held = true;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &UnavailableWorld,
        ));
        assert_eq!(physics.network_position(), Some(position));
        let sample = physics.sample_at(101 + index).unwrap();
        assert_eq!(sample.movement, [0.0; 3]);
        assert_eq!(sample.velocity, [0.0; 3]);
        assert!(!sample.grounded_after_tick);
        assert!(sample.jumping);
        assert!(!sample.processed.jump_initiated);
        assert!(!sample.processed.jump_arc_active);
        assert_eq!(sample.move_vector, [0.0, 1.0]);
        assert_eq!(sample.raw_move_vector, [0.25, 1.0]);
        assert_eq!(sample.analogue_move_vector, [0.25, 1.0]);
        assert_eq!(sample.pitch, index as f32 * 5.0);
        assert_eq!(sample.yaw, index as f32 * 10.0);
    }
    let snapshots = ticker.pending_snapshots();
    assert_eq!(snapshots.len(), 4);
    for packet in snapshots {
        assert_eq!(packet.position, position);
        assert_eq!(packet.delta, [0.0; 3]);
        assert_ne!(
            packet.flags.bits() & PlayerInputFlags::JUMP_CURRENT_RAW.bits(),
            0
        );
        assert_eq!(
            packet.flags.bits() & PlayerInputFlags::START_JUMPING.bits(),
            0
        );
    }
}

#[test]
fn immobile_ticks_suppress_pending_anchor_depenetration() {
    let mut physics = LocalPhysicsController::default();
    let position = [0.0, 0.5 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    physics.reanchor_network_position(position, 100, true);
    let frame = physics.advance(
        Duration::from_millis(50),
        MovementInput {
            immobile: true,
            ..MovementInput::default()
        },
        &VersionedFloor(1),
    );
    assert_eq!(frame.completed_ticks, 1, "{:?}", frame.blocked);
    assert_eq!(physics.network_position(), Some(position));
    assert_eq!(frame.samples[0].movement, [0.0; 3]);
}

#[test]
fn delayed_immobile_flag_replays_stationary_ticks_and_rewrites_unsent_positions() {
    let (mut on_time, _) = walked_physics(2);
    for _ in 0..2 {
        run_tick_with(
            &mut on_time,
            MovementInput {
                immobile: true,
                ..forward_physics_input()
            },
        );
    }
    let (mut delayed, mut ticker) = walked_physics(4);
    assert_eq!(
        delayed.apply_server_movement_flags(102, flags(|flags| flags.immobile = Some(true))),
        Some(102),
    );
    reconcile_timeline_rewind(&mut ticker, &mut delayed, 102, &VersionedFloor(1)).unwrap();
    assert_eq!(delayed.state(), on_time.state());
    for tick in [103, 104] {
        let sample = delayed.sample_at(tick).unwrap();
        assert_eq!(sample.velocity, [0.0; 3]);
        assert_eq!(sample.movement, [0.0; 3]);
    }
    assert_eq!(
        ticker.pending_snapshots().last().unwrap().position,
        on_time.network_position().unwrap()
    );
    let mut expected = on_time.clone();
    run_tick_with(&mut expected, MovementInput::default());
    run_tick_with(&mut delayed, MovementInput::default());
    assert_eq!(
        delayed.state(),
        expected.state(),
        "release starts with zero retained velocity"
    );
}

#[test]
fn delayed_immobile_clear_preserves_later_authoritative_freezes() {
    let (mut physics, mut ticker) = walked_physics(0);
    for (index, immobile) in [true, true, false, true, true].into_iter().enumerate() {
        run_tick_with(
            &mut physics,
            MovementInput {
                immobile,
                ..forward_physics_input()
            },
        );
        ticker
            .enqueue_completed_physics(physics.sample_at(101 + index as u64).unwrap().clone())
            .unwrap();
    }
    assert_eq!(
        physics.apply_server_movement_flags(101, flags(|flags| flags.immobile = Some(false))),
        Some(101),
    );
    reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
    let (mut expected, _) = walked_physics(0);
    for immobile in [true, false, false, true, true] {
        run_tick_with(
            &mut expected,
            MovementInput {
                immobile,
                ..forward_physics_input()
            },
        );
    }
    assert_eq!(physics.state(), expected.state());
    assert_ne!(physics.sample_at(102).unwrap().movement, [0.0; 3]);
    assert_eq!(physics.sample_at(104).unwrap().movement, [0.0; 3]);
    assert_eq!(physics.sample_at(105).unwrap().movement, [0.0; 3]);
}

#[test]
fn immobility_retiming_does_not_accept_an_unrelated_collision_registry() {
    let (mut physics, mut ticker) = walked_physics(4);
    physics.apply_server_movement_flags(102, flags(|flags| flags.immobile = Some(true)));
    assert!(reconcile_timeline_rewind(&mut ticker, &mut physics, 102, &VersionedFloor(2)).is_err());
}
