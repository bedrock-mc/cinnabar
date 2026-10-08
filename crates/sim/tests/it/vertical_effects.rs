use sim::{
    Aabb, CollisionQuery, CollisionWorld, MovementEffects, MovementInput, MovementMode,
    PlayerState, Simulator, Vec3, VerticalPhysics, WorldQueryError,
};

struct EmptyWorld;

impl CollisionWorld for EmptyWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }
}

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= f64::from(f32::EPSILON) * expected.abs().max(1.0),
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

        let lift = amplifier.wrapping_add(1) as f32 * 0.01_f32;
        assert_eq!(
            state.velocity.y,
            f64::from((-0.4_f32 * 0.8_f32 + lift) * 0.98_f32)
        );
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
    for (amplifier, lift) in [(0, 0.01), (3, 0.04)] {
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

        assert_close(state.velocity.y, (-0.4 * 0.8 + lift) * 0.98);
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

fn airborne_tick(
    vertical_physics: VerticalPhysics,
    effects: MovementEffects,
    mode: MovementMode,
) -> f64 {
    let mut state = PlayerState::new(Vec3::new(0.0, 8.0, 0.0));
    state.velocity.y = -0.25;
    Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                vertical_physics,
                effects,
                mode,
                ..MovementInput::default()
            },
            &EmptyWorld,
        )
        .unwrap();
    state.velocity.y
}

fn physics(has_gravity: bool, uniform_air_drag: bool, modifier: Option<f64>) -> VerticalPhysics {
    VerticalPhysics {
        has_gravity,
        uniform_air_drag,
        air_drag_modifier: modifier,
    }
}

#[test]
fn server_cleared_gravity_skips_gravity_and_vertical_drag() {
    let walking = MovementMode::Walking;
    let none = MovementEffects::default();
    assert_eq!(
        airborne_tick(physics(false, false, None), none, walking),
        -0.25
    );
    let levitation = MovementEffects {
        levitation: Some(0),
        ..MovementEffects::default()
    };
    assert_eq!(
        airborne_tick(physics(false, false, None), levitation, walking),
        f64::from(-0.25_f32 * 0.8_f32 + 0.01_f32)
    );
}

#[test]
fn uniform_air_drag_replaces_gravity_drag_with_its_own_retention() {
    let none = MovementEffects::default();
    let walking = MovementMode::Walking;
    assert_eq!(
        airborne_tick(physics(true, true, None), none, walking),
        f64::from((-0.25_f32 - 0.08_f32) * 0.91_f32)
    );
    // Uniform drag needs no gravity flag; the actor then only drags.
    assert_eq!(
        airborne_tick(physics(false, true, None), none, walking),
        f64::from(-0.25_f32 * 0.91_f32)
    );
}

#[test]
fn air_drag_modifier_scales_and_clamps_the_vertical_drag_fraction() {
    let none = MovementEffects::default();
    let walking = MovementMode::Walking;
    let fall = -0.25_f32 - 0.08_f32;
    for (modifier, retention) in [
        (2.0_f32, 1.0 - (1.0 - 0.98_f32) * 2.0),
        (0.5, 1.0 - (1.0 - 0.98_f32) * 0.5),
        (-3.0, 1.0),
        (60.0, 0.0),
    ] {
        assert_eq!(
            airborne_tick(
                physics(true, false, Some(f64::from(modifier))),
                none,
                walking
            ),
            f64::from(fall * retention),
            "modifier {modifier}"
        );
    }
    assert_eq!(
        airborne_tick(physics(true, false, Some(1.0)), none, walking),
        f64::from(fall * 0.98_f32)
    );
}

#[test]
fn air_drag_modifier_scales_flight_vertical_drag() {
    let none = MovementEffects::default();
    let friction = f32::from_bits(0x3ecc_cccc);
    assert_eq!(
        airborne_tick(physics(true, false, None), none, MovementMode::Flying),
        f64::from(-0.25_f32 * (1.0 - friction))
    );
    assert_eq!(
        airborne_tick(physics(true, false, Some(2.0)), none, MovementMode::Flying),
        f64::from(-0.25_f32 * (1.0 - friction * 2.0))
    );
    assert_eq!(
        airborne_tick(physics(true, false, Some(3.0)), none, MovementMode::Flying),
        0.0
    );
}

#[test]
fn vertical_physics_defaults_to_the_vanilla_player_and_is_omitted_when_default() {
    assert!(VerticalPhysics::default().has_gravity);
    let encoded = serde_json::to_string(&MovementInput::default()).unwrap();
    assert!(!encoded.contains("vertical_physics"), "{encoded}");
    let decoded: MovementInput =
        serde_json::from_str(r#"{"strafe":0,"forward":0,"yaw_degrees":0,"jumping":false,"jump_pressed":false,"sprinting":false,"sneaking":false,"vertical_physics":{"air_drag_modifier":2.0}}"#)
            .unwrap();
    assert_eq!(decoded.vertical_physics, physics(true, false, Some(2.0)));
}

#[test]
fn non_finite_air_drag_modifier_is_rejected() {
    let mut state = PlayerState::new(Vec3::new(0.0, 8.0, 0.0));
    let result = Simulator::default().tick(
        &mut state,
        MovementInput {
            vertical_physics: physics(true, false, Some(f64::NAN)),
            ..MovementInput::default()
        },
        &EmptyWorld,
    );
    assert!(result.is_err());
}
