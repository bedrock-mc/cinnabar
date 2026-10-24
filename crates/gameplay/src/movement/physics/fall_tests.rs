//! Fall-distance ownership across spatial corrections and retained replay.

use super::*;

struct Air;

impl CollisionWorld for Air {
    /// Leaves the falling player airborne throughout the correction.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

/// Captures a fall whose latest live distance differs from the correction anchor.
fn falling_controller() -> LocalPhysicsController {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.5, 40.0, 0.5], 100, false);
    for _ in 0..12 {
        let frame = physics.advance_with_context(
            simulation_clock::TICK_DURATION,
            MovementInput::default(),
            PhysicsSampleContext::default(),
            &Air,
        );
        assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
        assert_eq!(frame.samples.len(), 1);
    }
    assert!(physics.state().unwrap().fall_distance() > 3.0);
    physics
}

#[test]
fn correction_preserves_live_fall_distance_while_refreshing_history() {
    let mut physics = falling_controller();
    let before = physics.state().unwrap().clone();
    let tick = before.tick - 2;
    let anchor = physics.history.state_at(tick).unwrap();
    let network_position = [
        anchor.position.x as f32,
        anchor.position.y as f32 + PLAYER_NETWORK_OFFSET,
        anchor.position.z as f32,
    ];
    let plan = physics
        .apply_correction(
            crate::movement::PhysicsAnchor {
                network_position,
                tick,
                on_ground: false,
                velocity: Some([0.0, 0.5, 0.0]),
            },
            PhysicsCorrectionMode::ReplayIfRetained,
            None,
            &Air,
        )
        .unwrap();
    assert_eq!(plan.replayed_samples.len(), 2);
    assert!(physics.state().unwrap().position.y > before.position.y);
    assert_eq!(
        physics
            .history
            .state_at(before.tick)
            .unwrap()
            .fall_distance(),
        0.0
    );
    assert_eq!(
        physics.state().unwrap().fall_distance(),
        before.fall_distance()
    );
}

#[test]
fn spatial_snap_preserves_live_fall_distance() {
    let mut physics = falling_controller();
    let before = physics.state().unwrap().clone();
    physics
        .apply_correction(
            crate::movement::PhysicsAnchor {
                network_position: [8.5, 30.0, 8.5],
                tick: before.tick,
                on_ground: false,
                velocity: None,
            },
            PhysicsCorrectionMode::Snap,
            None,
            &Air,
        )
        .unwrap();
    assert_eq!(
        physics.state().unwrap().fall_distance(),
        before.fall_distance()
    );
}

#[test]
fn flight_actions_clear_fall_distance_before_movement() {
    let physics = falling_controller();
    let original = physics.state().unwrap().clone();
    let mut state = original.clone();
    let mut frame = *physics.controller_history.back().unwrap();
    let mut modes = ModeTracker::default();
    let mut anchor = AnchorProbeState::new();
    let mut input = frame.input;
    frame.intent = ModeIntent {
        can_fly: true,
        ..Default::default()
    };
    for expected in [true, false] {
        state.keep_fall_distance(&original);
        for jumping in [false, true, false, true] {
            input.jumping = jumping;
            frame.jump_edge = jumping;
            frame.jump_down = jumping;
            frame
                .prepare(
                    &mut modes,
                    &mut anchor,
                    sim::MovementEnvironment::default(),
                    &mut state,
                    &mut input,
                    &Air,
                )
                .unwrap();
        }
        assert_eq!(frame.flight_request, Some(expected));
        assert_eq!(state.fall_distance(), 0.0, "flying={expected}");
    }
}
