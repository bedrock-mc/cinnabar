#[test]
fn slowed_primary_preserves_captured_device_normalized_direction_policy() {
    let component = std::f32::consts::FRAC_1_SQRT_2;
    let diagonal_mask = PlayerInputFlags::UP_LEFT
        | PlayerInputFlags::UP_RIGHT
        | PlayerInputFlags::DOWN_LEFT
        | PlayerInputFlags::DOWN_RIGHT;
    for (axes, expected_diagonal) in [
        ([component, component], PlayerInputFlags::UP_RIGHT),
        ([0.5, 0.5], PlayerInputFlags::NONE),
        ([0.25, 0.5], PlayerInputFlags::NONE),
        ([0.0, 0.0], PlayerInputFlags::NONE),
    ] {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.0, 2.620_01, 0.0], 40, true);
        let input = physics_movement_input(axes, 0.0, true, false, true, false, None);
        let frame = physics.advance_with_context(
            Duration::from_millis(50),
            input,
            PhysicsSampleContext {
                raw_move_vector: [-1.0, -1.0],
                analogue_move_vector: [-1.0, -1.0],
                ..Default::default()
            },
            &Floor,
        );
        let [sample] = frame.samples.as_slice() else {
            panic!("expected one tick")
        };
        assert_eq!(
            sample.move_vector.map(f32::to_bits),
            axes.map(|axis| (axis * 0.3_f32).to_bits())
        );
        let mut ticker = MovementTicker::default();
        ticker.reset(1, 40, sample.position);
        ticker.set_source(MovementSource::Physics);
        ticker.enqueue_completed_physics(sample.clone()).unwrap();
        let snapshot = ticker.pop_pending().unwrap().snapshot;
        assert_eq!(
            snapshot.flags.bits() & diagonal_mask.bits(),
            expected_diagonal.bits()
        );
        assert_eq!(
            snapshot.flags.bits() & PlayerInputFlags::UP.bits() != 0,
            axes[1] > 0.0
        );
        assert_eq!(
            snapshot.flags.bits() & PlayerInputFlags::RIGHT.bits() != 0,
            axes[0] > 0.0
        );
        assert_eq!(
            snapshot.flags.bits() & (PlayerInputFlags::DOWN | PlayerInputFlags::LEFT).bits(),
            0
        );
    }
}

#[test]
fn captured_direction_snapshot_cannot_inject_unrelated_flags() {
    let mut sample = completed_sample(41, [0.0, 64.0, 0.0]);
    sample.move_vector = [0.0; 2];
    sample.processed.direction_flags = Some(
        PlayerInputFlags::UP_RIGHT
            | PlayerInputFlags::SPRINTING
            | PlayerInputFlags::JUMP_PRESSED_RAW,
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 40, sample.position);
    ticker.set_source(MovementSource::Physics);
    ticker.enqueue_completed_physics(sample).unwrap();
    let flags = ticker.pop_pending().unwrap().snapshot.flags;
    assert_ne!(flags.bits() & PlayerInputFlags::UP_RIGHT.bits(), 0);
    assert_eq!(
        flags.bits() & (PlayerInputFlags::SPRINTING | PlayerInputFlags::JUMP_PRESSED_RAW).bits(),
        0
    );
}

#[test]
fn partial_sneak_controls_are_scaled_once_before_packet_sampling() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 40, true);
    let input = physics_movement_input([0.25, 0.5], 0.0, true, false, true, false, None);
    let frame = physics.advance_with_context(
        Duration::from_millis(50),
        input,
        PhysicsSampleContext {
            raw_move_vector: [0.25, 0.5],
            analogue_move_vector: [0.25, 0.5],
            ..PhysicsSampleContext::default()
        },
        &Floor,
    );
    let [sample] = frame.samples.as_slice() else {
        panic!("expected one completed tick");
    };
    assert_eq!(sample.move_vector, [0.075, 0.15]);
    assert_eq!(sample.raw_move_vector, [0.25, 0.5]);
    assert_eq!(sample.analogue_move_vector, [0.25, 0.5]);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 40, sample.position);
    ticker.set_source(MovementSource::Physics);
    ticker.enqueue_completed_physics(sample.clone()).unwrap();
    assert_eq!(
        ticker.pop_pending().unwrap().snapshot.move_vector,
        [-0.075, 0.15]
    );
}

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
        analogue_move_vector: [0.0, 0.0],
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
            -std::f32::consts::FRAC_1_SQRT_2,
            std::f32::consts::FRAC_1_SQRT_2
        ]
    );
    assert_eq!(keyboard_snapshot.raw_move_vector, keyboard_snapshot.move_vector);
    assert_eq!(keyboard_snapshot.analogue_move_vector, [0.0, 0.0]);

    let gamepad_snapshot = ticker.pop_pending().unwrap().snapshot;
    assert_eq!(gamepad_snapshot.tick, 42);
    assert!((gamepad_snapshot.move_vector[0] + gamepad_style.move_vector[0]).abs() < 1e-6);
    assert!((gamepad_snapshot.move_vector[1] - gamepad_style.move_vector[1]).abs() < 1e-6);
    assert_eq!(gamepad_snapshot.raw_move_vector, [-0.6, 0.8]);
    assert_eq!(gamepad_snapshot.analogue_move_vector, [-0.6, 0.8]);
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


#[test]
fn digital_gamepad_direction_uses_the_normalized_raw_fallback() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 40, [0.0, 64.0, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker.enqueue_completed_physics(PhysicsMovementSample {
        raw_move_vector: [1.0, 1.0],
        analogue_move_vector: [0.0, 0.0],
        input_mode: PlayerInputMode::GamePad,
        ..completed_sample(41, [0.0, 64.0, 0.0])
    }).unwrap();
    let snapshot = ticker.pop_pending().unwrap().snapshot;
    assert_eq!(snapshot.analogue_move_vector, [0.0, 0.0]);
    assert_eq!(snapshot.raw_move_vector,
        [-std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2]);
}

#[test]
fn keyboard_neutral_vectors_preserve_positive_zero_on_the_wire() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 40, [0.0, 64.0, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(PhysicsMovementSample {
            move_vector: [0.0; 2],
            raw_move_vector: [0.0; 2],
            analogue_move_vector: [0.0; 2],
            ..completed_sample(41, [0.0, 64.0, 0.0])
        })
        .unwrap();
    let snapshot = ticker.pop_pending().unwrap().snapshot;
    for vector in [
        snapshot.move_vector,
        snapshot.raw_move_vector,
        snapshot.analogue_move_vector,
    ] {
        assert_eq!(vector.map(f32::to_bits), [0.0_f32.to_bits(); 2]);
    }
}
