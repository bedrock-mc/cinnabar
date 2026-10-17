/// Server-enabled flight remains usable when the server has not granted MayFly.
#[test]
fn server_forced_flight_ascends_without_may_fly() {
    let world = VersionedFloor(1);
    let intent = ModeIntent {
        can_fly: false,
        flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(true),
        creative_flight: true,
        ability_sequence: Some(1),
        ..Default::default()
    };
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(
        [0.0, 90.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0],
        100,
        false,
    );
    let first = step(&mut physics, MovementInput::default(), intent, &world);
    assert_eq!(first.processed.mode, MovementMode::Flying);
    let ascend = step(
        &mut physics,
        MovementInput {
            jumping: true,
            ..Default::default()
        },
        intent,
        &world,
    );
    assert!(ascend.position[1] > first.position[1]);
    assert!(has(
        input_flags(&ascend, HeldInput::from(&first)),
        PlayerInputFlags::WANT_UP
    ));
    let cleared = ModeIntent {
        flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(false),
        ability_sequence: Some(2),
        ..intent
    };
    let stopped = step(&mut physics, MovementInput::default(), cleared, &world);
    assert_eq!(stopped.processed.mode, MovementMode::Walking);
    let ungranted = step(
        &mut physics,
        MovementInput {
            jumping: true,
            ..Default::default()
        },
        cleared,
        &world,
    );
    assert_eq!(ungranted.processed.mode, MovementMode::Walking);
}

/// An actor's crouch flag cannot replace the keyboard's held flight-descent request.
#[test]
fn server_sneak_clear_preserves_held_flight_descent_during_replay() {
    let world = VersionedFloor(1);
    let intent = ModeIntent {
        can_fly: true,
        flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(true),
        creative_flight: true,
        ..Default::default()
    };
    let descend = MovementInput {
        sneaking: true,
        ..Default::default()
    };
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(
        [0.0, 90.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0],
        100,
        false,
    );
    let mut samples = Vec::new();
    for _ in 0..4 {
        samples.push(step(&mut physics, descend, intent, &world));
    }
    let expected = physics.state().unwrap().clone();
    let flags = client_world::MovementFlagUpdate {
        sneaking: Some(false),
        ..Default::default()
    };
    if let Some(tick) = physics.apply_server_movement_flags(samples[1].tick, flags) {
        physics.replay_retained_from(tick, &world).unwrap();
    }
    assert_eq!(physics.state().unwrap().position, expected.position);
    assert_eq!(physics.state().unwrap().velocity, expected.velocity);
    let continued = step(&mut physics, descend, intent, &world);
    assert_eq!(continued.processed.mode, MovementMode::Flying);
    assert!(continued.position[1] < samples.last().unwrap().position[1]);
    assert!(has(
        input_flags(&continued, HeldInput::from(samples.last().unwrap())),
        PlayerInputFlags::WANT_DOWN
    ));
}

/// A live pose clear must not become a latch update that releases flight descent.
#[test]
fn live_server_sneak_clear_does_not_release_flight_descent() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(
        [0.0, 90.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0],
        100,
        false,
    );
    let intent = ModeIntent {
        can_fly: true,
        flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(true),
        creative_flight: true,
        ..Default::default()
    };
    let descend = MovementInput {
        sneaking: true,
        ..Default::default()
    };
    step(&mut physics, descend, intent, &VersionedFloor(1));
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            sneaking: Some(false),
            ..Default::default()
        },
    );
    assert!(physics.take_server_control_flags().is_none());
    let continued = step(&mut physics, descend, intent, &VersionedFloor(1));
    assert!(continued.movement[1] < 0.0);
}

/// An older crouch correction stops at the transition where sneak starts controlling descent.
#[test]
fn walking_sneak_correction_does_not_rewrite_later_flight_input() {
    let world = VersionedFloor(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(
        [0.0, 90.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0],
        100,
        false,
    );
    let input = MovementInput {
        sneaking: true,
        ..Default::default()
    };
    let walking = step(&mut physics, input, ModeIntent::default(), &world);
    let flight = ModeIntent {
        can_fly: true,
        flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(true),
        creative_flight: true,
        ..Default::default()
    };
    for _ in 0..3 {
        step(&mut physics, input, flight, &world);
    }
    let expected = physics.state().unwrap().clone();
    if let Some(tick) = physics.apply_server_movement_flags(
        walking.tick,
        client_world::MovementFlagUpdate {
            sneaking: Some(false),
            ..Default::default()
        },
    ) {
        physics.replay_retained_from(tick, &world).unwrap();
    }
    assert_eq!(physics.state().unwrap().position, expected.position);
    assert_eq!(physics.state().unwrap().velocity, expected.velocity);
    assert_eq!(physics.mode(), MovementMode::Flying);
}
