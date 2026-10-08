use sim::{
    Aabb, CollisionQuery, CollisionWorld, MovementInput, PlayerState, SimulationError, Simulator,
    Vec3, WorldQueryError,
};

struct Empty;
impl CollisionWorld for Empty {
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }
}

fn controls(input: MovementInput) -> [f32; 2] {
    Simulator::default()
        .tick_with_controls(&mut PlayerState::new(Vec3::ZERO), input, &Empty)
        .unwrap()
        .controls
        .move_vector
        .map(|axis| axis as f32)
}

#[test]
fn raw_nonbinary_controls_round_operands_and_pose_intermediates_as_f32() {
    let input = MovementInput {
        strafe: f64::from(0.7_f32),
        forward: f64::from(-0.9_f32),
        move_vector_is_raw: true,
        sneaking: true,
        ..Default::default()
    };
    assert_eq!(controls(input)[0].to_bits(), 0x3e57_0a3e);
    let composed = controls(MovementInput {
        item_use_movement_modifier: Some(f64::from(0.7_f32)),
        ..input
    });
    assert_eq!(
        composed.map(f32::to_bits),
        [
            (0.7_f32 * 0.3_f32 * 0.7_f32).to_bits(),
            (-0.9_f32 * 0.3_f32 * 0.7_f32).to_bits()
        ]
    );
}

#[test]
fn raw_partial_controls_scale_while_historical_controls_clamp() {
    let input = MovementInput {
        strafe: 0.25,
        forward: -0.5,
        sneaking: true,
        ..Default::default()
    };
    assert_eq!(controls(input), [0.25, -0.3]);
    assert_eq!(
        controls(MovementInput {
            move_vector_is_raw: true,
            ..input
        }),
        [0.075, -0.15]
    );
    assert_eq!(
        controls(MovementInput {
            strafe: 2.0,
            forward: -2.0,
            move_vector_is_raw: true,
            ..input
        }),
        [0.3, -0.3]
    );
}

#[test]
fn explicit_item_modifier_overrides_flags_once_then_composes_pose() {
    let input = MovementInput {
        strafe: 0.25,
        forward: -0.5,
        move_vector_is_raw: true,
        using_consumable: true,
        ..Default::default()
    };
    assert_eq!(controls(input), [0.030625, -0.06125]);
    for (modifier, expected) in [
        (0.0, [0.0, 0.0]),
        (0.5, [0.125, -0.25]),
        (1.0, [0.25, -0.5]),
    ] {
        let actual = controls(MovementInput {
            item_use_movement_modifier: Some(modifier),
            ..input
        });
        assert_eq!(actual, expected);
    }
    assert_eq!(
        controls(MovementInput {
            sneaking: true,
            item_use_movement_modifier: Some(0.5),
            ..input
        }),
        [0.0375, -0.075]
    );
}

#[test]
fn invalid_item_modifier_rejects_before_world_queries_or_mutation() {
    struct Unqueried;
    impl CollisionWorld for Unqueried {
        fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            panic!("invalid input queried world")
        }
        fn block_physics(&self, _: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
            panic!("invalid input sampled world")
        }
    }
    for modifier in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        let mut state = PlayerState::new(Vec3::ZERO);
        let before = state.clone();
        assert_eq!(
            Simulator::default().tick_with_controls(
                &mut state,
                MovementInput {
                    item_use_movement_modifier: Some(modifier),
                    ..Default::default()
                },
                &Unqueried
            ),
            Err(SimulationError::InvalidItemUseMovementModifier)
        );
        assert_eq!(state, before);
    }
}

#[test]
fn historical_serialized_input_keeps_processed_semantics_and_result_schema() {
    let input: MovementInput = serde_json::from_str(r#"{"strafe":0.25,"forward":-0.5,"yaw_degrees":0.0,"jumping":false,"jump_pressed":false,"sprinting":false,"sneaking":true}"#).unwrap();
    assert!(!input.move_vector_is_raw);
    assert_eq!(controls(input), [0.25, -0.3]);
    let mut old_state = PlayerState::new(Vec3::ZERO);
    let mut new_state = old_state.clone();
    let simulator = Simulator::default();
    let old = simulator.tick(&mut old_state, input, &Empty).unwrap();
    let new = simulator
        .tick_with_controls(&mut new_state, input, &Empty)
        .unwrap();
    assert_eq!(old, new.tick_result);
    assert_eq!(old_state, new_state);
    assert!(!serde_json::to_string(&old).unwrap().contains("controls"));
}
