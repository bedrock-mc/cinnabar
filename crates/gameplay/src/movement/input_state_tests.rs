//! Packet requests and physical button events stay independent of simulated poses.

use super::*;
use semantic_input::ActionPhase;
use sim::MovementInput;
use std::time::Duration;

/// Encodes a sample without a previous actor action.
fn flags(sample: &PhysicsMovementSample) -> u128 {
    encoding::input_flags(sample, encoding::HeldInput::default()).bits()
}

/// An automatic sprint changes the actor without pressing the sprint control.
#[test]
fn actor_sprint_does_not_invent_a_sprint_button() {
    let mut sample = settle_tests::settled_sample(41, [0.0; 3]);
    sample.processed.sprinting = true;
    let bits = flags(&sample);
    assert_ne!(bits & PlayerInputFlags::START_SPRINTING.bits(), 0);
    assert_eq!(
        bits & (PlayerInputFlags::SPRINT_DOWN | PlayerInputFlags::SPRINTING).bits(),
        0
    );
    sample.input.sprint_down = true;
    sample.processed.sprinting = false;
    let bits = flags(&sample);
    assert_eq!(bits & PlayerInputFlags::START_SPRINTING.bits(), 0);
    let held = PlayerInputFlags::SPRINT_DOWN | PlayerInputFlags::SPRINTING;
    assert_eq!(bits & held.bits(), held.bits());
}

/// A ceiling changes the actor's stance without sending a sneak or descend request.
#[test]
fn forced_sneak_does_not_invent_sneak_request_lanes() {
    let mut sample = settle_tests::settled_sample(41, [0.0; 3]);
    sample.sneaking = true;
    sample.processed.sneaking = true;
    sample.processed.forced_sneak = true;
    let bits = flags(&sample);
    assert_ne!(bits & PlayerInputFlags::START_SNEAKING.bits(), 0);
    let requests = PlayerInputFlags::PERSIST_SNEAK
        | PlayerInputFlags::SNEAKING
        | PlayerInputFlags::SNEAK_DOWN
        | PlayerInputFlags::WANT_DOWN
        | PlayerInputFlags::SNEAK_CURRENT_RAW
        | PlayerInputFlags::SNEAK_PRESSED_RAW;
    assert_eq!(bits & requests.bits(), 0);
}

/// PersistSneak follows the input device's persistent controls, never the pose.
#[test]
fn persist_sneak_is_sent_for_touch_and_gamepad_only() {
    let mut sample = settle_tests::settled_sample(41, [0.0; 3]);
    for (mode, persists) in [
        (PlayerInputMode::Mouse, false),
        (PlayerInputMode::Touch, true),
        (PlayerInputMode::GamePad, true),
    ] {
        sample.input_mode = mode;
        assert_eq!(
            flags(&sample) & PlayerInputFlags::PERSIST_SNEAK.bits() != 0,
            persists,
            "{mode:?}"
        );
    }
}

/// Events from several render frames reach the next tick once, even if it ends released.
#[test]
fn a_tickless_tap_preserves_both_edges_and_does_not_repeat_them() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 40, true);
    let floor = integration_tests::VersionedFloor(1);
    let pressed = PhysicsSampleContext {
        input: TickInput {
            jump: ActionPhase {
                held: true,
                pressed: true,
                released: false,
            },
            sneak: ActionPhase {
                held: true,
                pressed: true,
                released: false,
            },
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        physics
            .advance_with_context(
                Duration::from_millis(10),
                MovementInput::default(),
                pressed,
                &floor
            )
            .completed_ticks,
        0
    );
    let released = PhysicsSampleContext {
        input: TickInput {
            jump: ActionPhase {
                released: true,
                ..Default::default()
            },
            sneak: ActionPhase {
                released: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let frame = physics.advance_with_context(
        Duration::from_millis(140),
        MovementInput::default(),
        released,
        &floor,
    );
    assert_eq!(frame.samples.len(), 3);
    let raw = PlayerInputFlags::JUMP_PRESSED_RAW
        | PlayerInputFlags::JUMP_RELEASED_RAW
        | PlayerInputFlags::SNEAK_PRESSED_RAW
        | PlayerInputFlags::SNEAK_RELEASED_RAW;
    assert_eq!(flags(&frame.samples[0]) & raw.bits(), raw.bits());
    let held = PlayerInputFlags::JUMP_CURRENT_RAW
        | PlayerInputFlags::JUMP_DOWN
        | PlayerInputFlags::SNEAK_CURRENT_RAW
        | PlayerInputFlags::SNEAK_DOWN;
    assert_eq!(flags(&frame.samples[0]) & held.bits(), 0);
    for sample in &frame.samples[1..] {
        assert_eq!(flags(sample) & raw.bits(), 0);
    }
    // Re-encoding an unsent sample leaves the one-shot events intact for a retry.
    assert_eq!(flags(&frame.samples[0]) & raw.bits(), raw.bits());
}

/// Missing terrain delays a button event until a tick can commit successfully.
#[test]
fn a_blocked_tick_keeps_raw_button_events_for_its_retry() {
    struct MissingTerrain;
    impl sim::CollisionWorld for MissingTerrain {
        fn collision_boxes(
            &self,
            _query: sim::Aabb,
        ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
            Err(sim::WorldQueryError::UnknownRuntimeId {
                runtime_id: 99,
                block: [0; 3],
            })
        }
    }
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 40, true);
    let context = PhysicsSampleContext {
        input: TickInput {
            jump: ActionPhase {
                pressed: true,
                released: true,
                held: false,
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let frame = physics.advance_with_context(
        Duration::from_millis(50),
        MovementInput::default(),
        context,
        &MissingTerrain,
    );
    assert!(frame.blocked.is_some());
    assert!(frame.samples.is_empty());
    let frame = physics.advance_with_context(
        Duration::from_millis(50),
        MovementInput::default(),
        PhysicsSampleContext::default(),
        &integration_tests::VersionedFloor(1),
    );
    assert_eq!(frame.samples.len(), 1);
    let edges = PlayerInputFlags::JUMP_PRESSED_RAW | PlayerInputFlags::JUMP_RELEASED_RAW;
    assert_eq!(flags(&frame.samples[0]) & edges.bits(), edges.bits());
    let next = physics.advance(
        Duration::from_millis(50),
        MovementInput::default(),
        &integration_tests::VersionedFloor(1),
    );
    assert_eq!(flags(&next.samples[0]) & edges.bits(), 0);
}

/// Tickless render frames cannot consume the forward timer, and catch-up/replay use the same latch.
#[test]
fn double_tap_sprint_uses_fixed_ticks_and_retains_its_latch_during_replay() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 40, true);
    let floor = integration_tests::VersionedFloor(1);
    let forward = physics_movement_input([0.0, 1.0], 0.0, true, false, false, false, None);
    let context = PhysicsSampleContext {
        input: TickInput {
            movement_buttons: semantic_input::MovementButtons {
                forward: true,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let first = physics
        .advance_with_context(Duration::from_millis(50), forward, context, &floor)
        .samples
        .remove(0);
    physics.advance(Duration::from_millis(50), MovementInput::default(), &floor);
    for _ in 0..30 {
        assert!(
            physics
                .advance(Duration::ZERO, MovementInput::default(), &floor)
                .samples
                .is_empty()
        );
    }
    let frame = physics.advance_with_context(Duration::from_millis(150), forward, context, &floor);
    assert_eq!(frame.samples.len(), 3);
    assert!(
        frame
            .samples
            .iter()
            .all(|sample| sample.processed.sprinting)
    );
    for sample in &frame.samples {
        assert_eq!(
            flags(sample) & (PlayerInputFlags::SPRINT_DOWN | PlayerInputFlags::SPRINTING).bits(),
            0
        );
    }
    let replay = physics
        .apply_correction(
            PhysicsAnchor {
                network_position: first.position,
                tick: first.tick,
                on_ground: true,
                velocity: Some(first.velocity),
            },
            PhysicsCorrectionMode::ReplayIfRetained,
            None,
            &floor,
        )
        .unwrap();
    let sprinting: Vec<_> = replay
        .replayed_samples
        .iter()
        .map(|sample| sample.processed.sprinting)
        .collect();
    assert_eq!(sprinting, [false, true, true, true]);
}

/// Item use narrows sprint intent before pose slowdown, which remains a later travel multiplier.
#[test]
fn item_slowdown_ends_a_sprint_but_crouch_slowdown_does_not() {
    for (item, sneak, expected) in [(Some(0.35), false, false), (None, true, true)] {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.0, 2.620_01, 0.0], 40, true);
        let floor = integration_tests::VersionedFloor(1);
        let context = PhysicsSampleContext {
            input: TickInput {
                sprint_down: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let sprint = physics_movement_input([0.0, 1.0], 0.0, true, false, false, true, None);
        let first = physics
            .advance_with_context(Duration::from_millis(50), sprint, context, &floor)
            .samples
            .remove(0);
        assert!(first.processed.sprinting);
        let input = physics_movement_input([0.0, 1.0], 0.0, true, false, sneak, true, item);
        let context = PhysicsSampleContext {
            input: TickInput {
                sprint_down: true,
                sneak_down: sneak,
                ..Default::default()
            },
            mode_intent: ModeIntent {
                sprint_start_blocked: item.is_some(),
                ..Default::default()
            },
            ..Default::default()
        };
        let next = physics
            .advance_with_context(Duration::from_millis(50), input, context, &floor)
            .samples
            .remove(0);
        assert_eq!(next.processed.sprinting, expected);
        let replay = physics
            .apply_correction(
                PhysicsAnchor {
                    network_position: first.position,
                    tick: first.tick,
                    on_ground: true,
                    velocity: Some(first.velocity),
                },
                PhysicsCorrectionMode::ReplayIfRetained,
                None,
                &floor,
            )
            .unwrap();
        let [replayed] = replay.replayed_samples.as_slice() else {
            panic!("expected one replayed tick")
        };
        assert_eq!(replayed.processed, next.processed);
        assert_eq!(replayed.input, next.input);
        assert_eq!(replayed.move_vector, next.move_vector);
        assert_eq!(replayed.position, next.position);
        assert_eq!(replayed.velocity, next.velocity);
    }
}
