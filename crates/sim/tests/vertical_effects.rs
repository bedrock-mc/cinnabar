use sim::{
    Aabb, CollisionQuery, CollisionWorld, MovementEffects, MovementInput, PlayerState, Simulator,
    Vec3, WorldQueryError,
};

struct EmptyWorld;

impl CollisionWorld for EmptyWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1.0e-12,
        "{actual} != {expected}"
    );
}

#[test]
fn jump_boost_uses_zero_based_amplifiers() {
    for (amplifier, expected_jump) in [
        (-6, 0.0),
        (-5, 0.02),
        (-2, 0.32),
        (-1, 0.42),
        (0, 0.52),
        (1, 0.62),
    ] {
        let mut state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
        state.on_ground = true;
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    jumping: true,
                    jump_pressed: true,
                    effects: MovementEffects {
                        jump_boost: Some(amplifier),
                        ..MovementEffects::default()
                    },
                    ..MovementInput::default()
                },
                &EmptyWorld,
            )
            .unwrap();

        assert_close(tick.movement.y, expected_jump);
        assert_close(state.velocity.y, (expected_jump - 0.08) * 0.98);
    }
}

#[test]
fn signed_levitation_matrix_reverses_and_extremes_remain_finite() {
    for amplifier in [i32::MIN, -4, -2, -1, 0, 3, i32::MAX] {
        let mut state = PlayerState::new(Vec3::new(0.0, 4.0, 0.0));
        state.velocity.y = -0.4;
        Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    effects: MovementEffects {
                        levitation: Some(amplifier),
                        ..MovementEffects::default()
                    },
                    ..MovementInput::default()
                },
                &EmptyWorld,
            )
            .unwrap();

        let target = 0.05 * (f64::from(amplifier) + 1.0);
        assert_close(state.velocity.y, -0.4 + (target - -0.4) * 0.2);
        assert!(state.velocity.is_finite());
    }
}

#[test]
fn extreme_positive_jump_boost_fails_transactionally_at_the_sweep_bound() {
    let mut state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    state.on_ground = true;
    let original = state.clone();

    let error = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                jump_pressed: true,
                effects: MovementEffects {
                    jump_boost: Some(i32::MAX),
                    ..MovementEffects::default()
                },
                ..MovementInput::default()
            },
            &EmptyWorld,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        sim::SimulationError::World(WorldQueryError::QueryExtentExceeded)
    ));
    assert_eq!(state, original);
}

#[test]
fn levitation_replaces_gravity_and_scales_from_amplifier_zero() {
    for (amplifier, target) in [(0, 0.05), (3, 0.20)] {
        let mut state = PlayerState::new(Vec3::new(0.0, 4.0, 0.0));
        state.velocity.y = -0.4;
        Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    effects: MovementEffects {
                        levitation: Some(amplifier),
                        slow_falling: true,
                        ..MovementEffects::default()
                    },
                    ..MovementInput::default()
                },
                &EmptyWorld,
            )
            .unwrap();

        assert_close(state.velocity.y, -0.4 + (target - -0.4) * 0.2);
    }
}

#[test]
fn slow_falling_reduces_gravity_only_while_descending() {
    let mut falling = PlayerState::new(Vec3::new(0.0, 4.0, 0.0));
    falling.velocity.y = -0.2;
    Simulator::default()
        .tick(
            &mut falling,
            MovementInput {
                effects: MovementEffects {
                    slow_falling: true,
                    ..MovementEffects::default()
                },
                ..MovementInput::default()
            },
            &EmptyWorld,
        )
        .unwrap();
    assert_close(falling.velocity.y, (-0.2 - 0.01) * 0.98);

    let mut rising = PlayerState::new(Vec3::new(0.0, 4.0, 0.0));
    rising.velocity.y = 0.2;
    Simulator::default()
        .tick(
            &mut rising,
            MovementInput {
                effects: MovementEffects {
                    slow_falling: true,
                    ..MovementEffects::default()
                },
                ..MovementInput::default()
            },
            &EmptyWorld,
        )
        .unwrap();
    assert_close(rising.velocity.y, (0.2 - 0.08) * 0.98);
}

#[test]
fn neutral_effect_snapshot_preserves_existing_motion_exactly() {
    let mut default_state = PlayerState::new(Vec3::new(0.0, 4.0, 0.0));
    default_state.velocity = Vec3::new(0.25, -0.2, -0.125);
    let mut explicit_neutral = default_state.clone();
    let simulator = Simulator::default();

    let default_tick = simulator
        .tick(&mut default_state, MovementInput::default(), &EmptyWorld)
        .unwrap();
    let neutral_tick = simulator
        .tick(
            &mut explicit_neutral,
            MovementInput {
                effects: MovementEffects::default(),
                ..MovementInput::default()
            },
            &EmptyWorld,
        )
        .unwrap();

    assert_eq!(neutral_tick, default_tick);
    assert_eq!(explicit_neutral, default_state);
}
