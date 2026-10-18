//! Deterministic camera contracts for tick sampling and numeric matrices.
use super::*;

/// A completed ordinary walking tick with deliberately different displacement and speed.
fn walking(tick: u64, x: f32) -> LegacyCameraTick {
    LegacyCameraTick {
        tick,
        position: [x, 64.0, 0.0],
        movement: [0.2, 0.0, 0.0],
        velocity: [0.3, -0.0784, 0.0],
        on_ground: true,
        alive: true,
        riding: false,
        walks: true,
    }
}

/// Reads a numeric fixture input, using zero for an absent optional counter.
fn number(inputs: &serde_json::Value, key: &str) -> f32 {
    inputs[key].as_f64().unwrap_or(0.0) as f32
}

#[test]
fn matrices_match_all_numeric_fixtures_and_keep_the_native_view_order() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures.json")).unwrap();
    assert_eq!(cases.len(), 36);
    let mut walking_matrix = Mat4::IDENTITY;
    for case in cases {
        let inputs = &case["inputs"];
        let alpha = number(inputs, "alpha");
        let actual = if case["kind"] == "walk" {
            let endpoints = LegacyCameraEndpoints {
                walked: [number(inputs, "previous_walk"), number(inputs, "walk")],
                bob: [number(inputs, "previous_bob"), number(inputs, "bob")],
                fall_degrees: [number(inputs, "previous_tilt"), number(inputs, "tilt")],
            };
            walking_matrix = endpoints.bob_matrix(alpha);
            walking_matrix
        } else {
            legacy_hurt_matrix(
                number(inputs, "hurt") as u8,
                number(inputs, "direction"),
                inputs.get("death").map(|_| number(inputs, "death")),
                alpha,
                1.0,
            )
        };
        let expected: [f32; 16] =
            std::array::from_fn(|index| case["matrix"][index].as_f64().unwrap() as f32);
        let expected = Mat4::from_cols_array(&expected);
        assert!(
            actual.abs_diff_eq(expected, 3e-6),
            "input {inputs}: {actual:?} != {expected:?}"
        );
        // Nonidentity look and translation distinguish the view-side multiplication order.
        let base = Mat4::from_rotation_y(0.7)
            * Mat4::from_rotation_x(-0.4)
            * Mat4::from_translation(Vec3::new(12.0, -6.0, 3.0));
        assert!((actual * base).abs_diff_eq(expected * base, 3e-5));
    }
    for direction in [-90.0, 90.0] {
        let hurt = legacy_hurt_matrix(5, direction, Some(12.0), 0.5, 1.0);
        let view = hurt * walking_matrix;
        assert!(!view.abs_diff_eq(walking_matrix * hurt, 1e-5));
        let base = Mat4::from_rotation_y(0.9) * Mat4::from_translation(Vec3::new(3.0, 4.0, -8.0));
        let rendered_pose = base.inverse() * view.inverse();
        assert!(rendered_pose.inverse().abs_diff_eq(view * base, 3e-5));
    }
}

#[test]
fn tick_state_uses_resolved_distance_caps_and_follow_rates() {
    let rules = BEDROCK_112;
    let mut state = LegacyCameraState::default();
    state.advance(walking(1, 0.0));
    assert_eq!(state.endpoints, LegacyCameraEndpoints::default());
    state.advance(walking(2, 0.2));
    assert_eq!(state.endpoints.walked, [0.0, rules.walk_gain * 0.2]);
    assert_eq!(state.endpoints.bob, [0.0, rules.bob_follow * rules.bob_cap]);
    let saved = state;
    for _ in 0..120 {
        state.advance(walking(2, 0.2));
        for alpha in [0.0, 0.25, 0.5, 1.0] {
            assert_eq!(
                state.endpoints.bob_matrix(alpha),
                saved.endpoints.bob_matrix(alpha)
            );
        }
    }
    assert_eq!(state, saved);
    state.advance(LegacyCameraTick {
        movement: [0.0; 3],
        ..walking(3, 0.2)
    });
    assert_eq!(state.endpoints.walked[1], saved.endpoints.walked[1]);
    // Obstruction keeps distance stationary even when a speed request remains nonzero.
    assert!(state.endpoints.bob[1] <= rules.bob_cap);
    let bob = state.endpoints.bob[1];
    state.advance(LegacyCameraTick {
        on_ground: false,
        walks: false,
        velocity: [0.0, -0.5, 0.0],
        ..walking(4, 0.2)
    });
    assert_eq!(state.endpoints.bob[1], bob + rules.bob_follow * -bob);
    let target_fall = rules.fall_degrees_gain * (rules.fall_velocity_gain * -0.5).atan();
    assert_eq!(
        state.endpoints.fall_degrees[1],
        rules.fall_follow * target_fall
    );
    state.advance(LegacyCameraTick {
        on_ground: false,
        walks: false,
        velocity: [0.0, 0.5, 0.0],
        ..walking(5, 0.2)
    });
    assert!(state.endpoints.fall_degrees[1] < 0.0);
}

#[test]
fn phase_extrapolates_while_amplitude_and_fall_interpolate() {
    let state = LegacyCameraEndpoints {
        walked: [0.63, 0.75],
        bob: [0.02, 0.06],
        fall_degrees: [0.0, 1.2],
    };
    for alpha in [0.0, 0.25, 0.5, 1.0] {
        let matrix = state.bob_matrix(alpha);
        let phase =
            -std::f32::consts::PI * (state.walked[1] + (state.walked[1] - state.walked[0]) * alpha);
        let bob = state.bob[0] + (state.bob[1] - state.bob[0]) * alpha;
        assert_eq!(
            matrix.w_axis.x,
            BEDROCK_112.horizontal_gain * bob * legacy_sin(phase)
        );
        assert_eq!(matrix.w_axis.y, -(bob * legacy_cos(phase)).abs());
    }
    assert_eq!(state.bob_matrix(f32::NAN), state.bob_matrix(0.0));
}

#[test]
fn riding_death_suppression_stationary_and_resets_are_tick_owned() {
    let mut state = LegacyCameraState::default();
    state.advance(walking(1, 0.0));
    state.advance(walking(2, 0.2));
    let walked = state.endpoints.walked[1];
    for tick in 3..8 {
        state.advance(LegacyCameraTick {
            alive: false,
            riding: true,
            walks: false,
            ..walking(tick, 0.2)
        });
        assert_eq!(state.endpoints.walked[1], walked);
        assert_eq!(state.endpoints.fall_degrees[1], 0.0);
    }
    assert!(state.endpoints.bob[1] < 0.004);
    state.advance(walking(8, 100.0));
    assert_eq!(state.endpoints, LegacyCameraEndpoints::default());
    state.advance(walking(1, 100.0));
    assert_eq!(state.endpoints, LegacyCameraEndpoints::default());
    state.advance(walking(5, 100.0));
    assert_eq!(state.endpoints, LegacyCameraEndpoints::default());
    state.advance(LegacyCameraTick {
        movement: [0.0; 3],
        velocity: [0.0; 3],
        ..walking(6, 100.0)
    });
    assert_eq!(state.endpoints.bob_matrix(0.5), Mat4::IDENTITY);
}

#[test]
fn hurt_strength_scales_the_angle_but_never_death_and_direction_rotates_the_axis() {
    let death = legacy_hurt_matrix(0, 0.0, Some(12.0), 0.5, 1.0);
    assert_eq!(legacy_hurt_matrix(5, 90.0, Some(12.0), 0.5, 0.0), death);
    assert_eq!(legacy_hurt_matrix(0, 0.0, None, 1.0, 1.0), Mat4::IDENTITY);
    let right = legacy_hurt_matrix(5, 0.0, None, 0.5, 1.0).transform_vector3(Vec3::X);
    assert!(right.y < 0.0);
    let front = legacy_hurt_matrix(5, 90.0, None, 0.5, 1.0).transform_vector3(Vec3::Y);
    let back = legacy_hurt_matrix(5, -90.0, None, 0.5, 1.0).transform_vector3(Vec3::Y);
    assert!(front.z.abs() > 0.01 && (front.z + back.z).abs() < 1e-6);
    assert_eq!(legacy_hurt_matrix(5, 0.0, None, 0.5, 0.0), Mat4::IDENTITY);
}

/// The module's full intensity range scales hurt alone, without amplifying death rotation.
#[test]
fn double_hurt_intensity_preserves_death_and_doubles_the_damage_angle() {
    let ordinary = legacy_hurt_matrix(5, 0.0, None, 0.5, 1.0);
    let doubled = legacy_hurt_matrix(5, 0.0, None, 0.5, render_api::MAX_HURT_CAMERA_INTENSITY);
    assert!(doubled.abs_diff_eq(ordinary * ordinary, 1e-5));
    assert!(!doubled.abs_diff_eq(ordinary, 1e-5));
    let death = legacy_hurt_matrix(0, 0.0, Some(12.0), 0.5, 1.0);
    assert_eq!(legacy_hurt_matrix(0, 0.0, Some(12.0), 0.5, 2.0), death);
    assert!(legacy_hurt_matrix(5, 0.0, Some(12.0), 0.5, 2.0).abs_diff_eq(death * doubled, 1e-5));
}
