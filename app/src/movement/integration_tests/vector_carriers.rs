#[test]
fn completed_samples_carry_the_context_move_vector_carriers() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 40, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext {
            raw_move_vector: [1.0, 0.8],
            analogue_move_vector: [0.6, 0.8],
            ..PhysicsSampleContext::default()
        },
        &Floor,
    );
    let [sample] = frame.samples.as_slice() else {
        panic!("expected exactly one completed physics tick");
    };
    assert_eq!(sample.move_vector, [0.0, 1.0]);
    assert_eq!(sample.raw_move_vector, [1.0, 0.8]);
    assert_eq!(sample.analogue_move_vector, [0.6, 0.8]);
}

#[test]
fn tick_snapshots_map_each_device_carrier_to_its_wire_field() {
    let keyboard_style = PhysicsMovementSample {
        move_vector: [1.0, 1.0],
        raw_move_vector: [1.0, 1.0],
        analogue_move_vector: [1.0, 1.0],
        ..completed_sample(41, [1.0, 64.0, 2.0])
    };
    let gamepad_style = PhysicsMovementSample {
        move_vector: [0.780_869_4, 0.624_695_04],
        raw_move_vector: [1.0, 0.8],
        analogue_move_vector: [0.6, 0.8],
        input_mode: PlayerInputMode::GamePad,
        ..completed_sample(42, [1.0, 64.0, 2.0])
    };

    let mut ticker = MovementTicker::default();
    ticker.reset(1, 40, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(keyboard_style.clone())
        .unwrap();
    ticker
        .enqueue_completed_physics(gamepad_style.clone())
        .unwrap();

    let keyboard_snapshot = ticker.pop_pending().unwrap().snapshot;
    assert_eq!(keyboard_snapshot.tick, 41);
    assert_eq!(
        keyboard_snapshot.move_vector,
        [
            std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2
        ]
    );
    assert_eq!(keyboard_snapshot.raw_move_vector, [1.0, 1.0]);
    assert_eq!(keyboard_snapshot.analogue_move_vector, [1.0, 1.0]);

    let gamepad_snapshot = ticker.pop_pending().unwrap().snapshot;
    assert_eq!(gamepad_snapshot.tick, 42);
    assert!((gamepad_snapshot.move_vector[0] - gamepad_style.move_vector[0]).abs() < 1e-6);
    assert!((gamepad_snapshot.move_vector[1] - gamepad_style.move_vector[1]).abs() < 1e-6);
    assert_eq!(gamepad_snapshot.raw_move_vector, [1.0, 0.8]);
    assert_eq!(gamepad_snapshot.analogue_move_vector, [0.6, 0.8]);
}

#[test]
fn direction_flags_ignore_raw_and_analogue_carriers() {
    let diagonal_mask = PlayerInputFlags::UP_LEFT.bits()
        | PlayerInputFlags::UP_RIGHT.bits()
        | PlayerInputFlags::DOWN_LEFT.bits()
        | PlayerInputFlags::DOWN_RIGHT.bits();
    let mut sample = completed_sample(43, [1.0, 64.0, 2.0]);
    sample.move_vector = [0.0, 1.0];
    sample.raw_move_vector = [-1.0, -1.0];
    sample.analogue_move_vector = [1.0, 1.0];

    let mut ticker = MovementTicker::default();
    ticker.reset(1, 42, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    ticker.enqueue_completed_physics(sample).unwrap();
    let snapshot = ticker.pop_pending().unwrap().snapshot;

    assert_ne!(snapshot.flags.bits() & PlayerInputFlags::UP.bits(), 0);
    assert_eq!(
        snapshot.flags.bits()
            & (PlayerInputFlags::DOWN | PlayerInputFlags::LEFT | PlayerInputFlags::RIGHT).bits(),
        0
    );
    assert_eq!(snapshot.flags.bits() & diagonal_mask, 0);
}

#[test]
fn non_finite_device_carriers_fail_physics_authority_closed() {
    for mutate in [
        |sample: &mut PhysicsMovementSample| sample.raw_move_vector[0] = f32::NAN,
        |sample: &mut PhysicsMovementSample| sample.analogue_move_vector[1] = f32::INFINITY,
    ] {
        let mut ticker = MovementTicker::default();
        ticker.reset(1, 41, [1.0, 64.0, 2.0]);
        ticker.set_source(MovementSource::Physics);
        let mut sample = completed_sample(42, [1.0, 64.0, 2.0]);
        mutate(&mut sample);

        assert_eq!(
            ticker.enqueue_completed_physics(sample),
            Err(PhysicsAuthorityFault::InvalidCompletedSample)
        );
        assert_eq!(ticker.source(), MovementSource::FreeCamera);
        assert_eq!(ticker.pending_count(), 0);
    }
}
