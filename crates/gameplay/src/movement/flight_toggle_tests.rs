use semantic_input::ActionPhase;

use super::*;

const HELD: ActionPhase = ActionPhase {
    held: true,
    pressed: false,
    released: false,
};
const UP: ActionPhase = ActionPhase {
    held: false,
    pressed: false,
    released: false,
};

/// Flight permission whose latest server ability update is `sequence`.
fn abilities(sequence: u64, server_flying: bool) -> ModeIntent {
    ModeIntent {
        can_fly: true,
        flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(server_flying),
        ability_sequence: Some(sequence),
        creative_flight: true,
        ..Default::default()
    }
}

/// Completes one fixed tick with explicit jump button phases and forward input.
fn tick(
    physics: &mut LocalPhysicsController,
    forward: f64,
    jump: ActionPhase,
    intent: ModeIntent,
) -> PhysicsMovementSample {
    let frame = physics.advance_with_context(
        TICK,
        MovementInput {
            forward,
            jumping: jump.held,
            ..Default::default()
        },
        PhysicsSampleContext {
            input: super::super::TickInput {
                jump,
                ..Default::default()
            },
            mode_intent: intent,
            ..Default::default()
        },
        &VersionedFloor(1),
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    frame.samples.into_iter().next().expect("one tick")
}

/// Double-taps from the ground and climbs `climb` ticks with jump held.
fn take_off(
    physics: &mut LocalPhysicsController,
    intent: ModeIntent,
    climb: usize,
) -> PhysicsMovementSample {
    for jump in [UP, HELD, UP] {
        tick(physics, 0.0, jump, intent);
    }
    let started = tick(physics, 0.0, HELD, intent);
    assert_eq!(started.processed.mode, MovementMode::Flying);
    assert!(has(
        input_flags(&started, HeldInput::default()),
        PlayerInputFlags::START_FLYING
    ));
    let mut last = started;
    for _ in 0..climb {
        last = tick(physics, 0.0, HELD, intent);
    }
    last
}

fn flight_flags(sample: &PhysicsMovementSample, previous: &PhysicsMovementSample) -> [bool; 2] {
    let flags = input_flags(sample, HeldInput::from(previous));
    [
        has(flags, PlayerInputFlags::START_FLYING),
        has(flags, PlayerInputFlags::STOP_FLYING),
    ]
}

/// The server sees only per-tick `JumpDown`/`Jumping`, so taps between ticks cannot toggle flight.
#[test]
fn jump_taps_between_ticks_do_not_count_toward_the_flight_double_tap() {
    let tap_inside_tick = ActionPhase {
        held: false,
        pressed: true,
        released: true,
    };
    let repress_inside_tick = ActionPhase {
        held: true,
        pressed: true,
        released: true,
    };
    for (first, second) in [(tap_inside_tick, HELD), (HELD, repress_inside_tick)] {
        let mut physics = grounded_controller();
        let intent = abilities(1, false);
        let mut previous = take_off(&mut physics, intent, 10);
        for jump in [UP, first, second] {
            let sample = tick(&mut physics, 0.0, jump, intent);
            assert_eq!(sample.processed.mode, MovementMode::Flying);
            assert_eq!(flight_flags(&sample, &previous), [false, false]);
            previous = sample;
        }
    }
}

/// Stopping mid-flight with forward held keeps momentum, falls without rebounding and lands.
#[test]
fn stopping_flight_while_moving_forward_falls_and_drifts_until_landing() {
    let mut physics = grounded_controller();
    let intent = abilities(1, false);
    take_off(&mut physics, intent, 20);
    let mut cruise = tick(&mut physics, 1.0, UP, intent);
    for _ in 0..10 {
        cruise = tick(&mut physics, 1.0, UP, intent);
    }
    let speed = |sample: &PhysicsMovementSample| sample.velocity[0].hypot(sample.velocity[2]);
    let first = tick(&mut physics, 1.0, HELD, intent);
    let gap = tick(&mut physics, 1.0, UP, intent);
    assert_eq!(gap.processed.mode, MovementMode::Flying);
    let stop = tick(&mut physics, 1.0, HELD, intent);
    assert_eq!(stop.processed.mode, MovementMode::Walking);
    assert_eq!(flight_flags(&first, &cruise), [false, false]);
    assert_eq!(flight_flags(&stop, &gap), [false, true]);
    assert!(
        speed(&stop) > 0.8 * speed(&gap),
        "stopping keeps horizontal momentum"
    );

    let mut previous = stop;
    for _ in 0..80 {
        let sample = tick(&mut physics, 1.0, UP, intent);
        assert_eq!(sample.processed.mode, MovementMode::Walking);
        assert_eq!(flight_flags(&sample, &previous), [false, false]);
        assert!(sample.movement[2] != 0.0 || sample.movement[0] != 0.0);
        if sample.grounded_after_tick {
            assert!(sample.position[1] < first.position[1]);
            return;
        }
        assert!(
            sample.velocity[1] < previous.velocity[1],
            "the fall never rebounds: {:?} after {:?}",
            sample.velocity,
            previous.velocity
        );
        previous = sample;
    }
    panic!("the player never landed");
}

/// A late server resync repeating the earlier flying value restores flight without a start edge.
#[test]
fn a_late_resync_with_an_unchanged_flying_value_restores_server_flight() {
    let mut physics = grounded_controller();
    let mut previous = tick(&mut physics, 0.0, UP, abilities(1, true));
    assert_eq!(previous.processed.mode, MovementMode::Flying);
    for _ in 0..10 {
        previous = tick(&mut physics, 0.0, HELD, abilities(1, true));
    }
    let mut stopped_at = None;
    for (index, jump) in [UP, HELD, UP, HELD, UP, UP].into_iter().enumerate() {
        let sample = tick(&mut physics, 0.0, jump, abilities(1, true));
        if flight_flags(&sample, &previous) == [false, true] {
            stopped_at = Some(index);
        }
        previous = sample;
    }
    assert_eq!(stopped_at, Some(3));
    assert_eq!(previous.processed.mode, MovementMode::Walking);
    let resync = tick(&mut physics, 0.0, UP, abilities(2, true));
    assert_eq!(resync.processed.mode, MovementMode::Flying);
    assert_eq!(flight_flags(&resync, &previous), [false, false]);
    let hover = tick(&mut physics, 0.0, UP, abilities(2, true));
    assert_eq!(hover.processed.mode, MovementMode::Flying);
    assert!(hover.velocity[1] > resync.velocity[1]);
}

/// Flight survives landing; after a local stop, a double-tap from the ground jumps, then flies up.
#[test]
fn flight_survives_landing_and_a_ground_double_tap_jumps_before_taking_off() {
    let mut physics = grounded_controller();
    let intent = abilities(1, false);
    take_off(&mut physics, intent, 4);
    let mut previous = tick(&mut physics, 0.0, UP, intent);
    for _ in 0..60 {
        let frame = physics.advance_with_context(
            TICK,
            MovementInput {
                sneaking: true,
                ..Default::default()
            },
            PhysicsSampleContext {
                input: super::super::TickInput {
                    sneak_down: true,
                    ..Default::default()
                },
                mode_intent: intent,
                ..Default::default()
            },
            &VersionedFloor(1),
        );
        let sample = frame.samples.into_iter().next().expect("one tick");
        assert_eq!(sample.processed.mode, MovementMode::Flying);
        assert_eq!(flight_flags(&sample, &previous), [false, false]);
        previous = sample;
    }
    assert!(previous.grounded_after_tick, "descent reaches the floor");
    for jump in [HELD, UP, HELD] {
        previous = tick(&mut physics, 0.0, jump, intent);
    }
    assert_eq!(previous.processed.mode, MovementMode::Walking);
    for _ in 0..30 {
        previous = tick(&mut physics, 0.0, UP, intent);
    }
    assert!(previous.grounded_after_tick);

    let jumped = tick(&mut physics, 0.0, HELD, intent);
    assert!(jumped.processed.jump_initiated);
    assert_eq!(jumped.processed.mode, MovementMode::Walking);
    let gap = tick(&mut physics, 0.0, UP, intent);
    let started = tick(&mut physics, 0.0, HELD, intent);
    assert_eq!(started.processed.mode, MovementMode::Flying);
    assert_eq!(flight_flags(&started, &gap), [true, false]);
    let mut previous = started;
    for _ in 0..10 {
        let sample = tick(&mut physics, 0.0, HELD, intent);
        assert_eq!(sample.processed.mode, MovementMode::Flying);
        assert_eq!(flight_flags(&sample, &previous), [false, false]);
        assert!(sample.position[1] > previous.position[1]);
        previous = sample;
    }
}
