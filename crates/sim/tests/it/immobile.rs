use sim::{
    Aabb, BlockPhysicsSample, CollisionQuery, CollisionRegistryIdentity, CollisionWorld,
    MotionOverlay, MovementInput, MovementMode, PlayerState, PredictionHistory, SimulationError,
    Simulator, Vec3, WorldQueryError,
};

struct Unqueried;

impl CollisionWorld for Unqueried {
    fn registry_identity(&self) -> CollisionRegistryIdentity {
        let mut identity = CollisionQuery::synthetic(()).identity.registry;
        identity.preg_sha256 = [7; 32];
        identity
    }

    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        panic!("an immobile tick must not read collision data")
    }

    fn block_physics(&self, _: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        panic!("an immobile tick must not sample block physics")
    }

    fn liquid_current(&self, _: Aabb) -> Result<Option<CollisionQuery<Vec3>>, WorldQueryError> {
        panic!("an immobile tick must not sample liquid current")
    }

    fn primary_is_air(&self, _: [i32; 3]) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        panic!("an immobile tick must not sample air")
    }
}

struct Empty;

impl CollisionWorld for Empty {
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }
}

fn moving_frozen_input() -> MovementInput {
    MovementInput {
        immobile: true,
        strafe: 1.0,
        forward: -1.0,
        jumping: true,
        jump_pressed: true,
        sprinting: true,
        move_vector_is_raw: true,
        ..MovementInput::default()
    }
}

#[test]
fn immobile_freezes_airborne_velocity_and_jump_without_rounding_or_terrain_reads() {
    let position = Vec3::new(1.000_000_000_000_000_2, 17.123_456_789, -3.987_654_321);
    for on_ground in [false, true] {
        for mode in [
            MovementMode::Walking,
            MovementMode::Flying,
            MovementMode::Swimming,
            MovementMode::Crawling,
            MovementMode::Gliding,
            MovementMode::Riding,
        ] {
            let mut state = PlayerState::new(position);
            state.tick = 42;
            state.on_ground = on_ground;
            state.velocity = Vec3::new(0.7, -1.3, -0.9);
            state.movement = Vec3::new(0.1, -0.3, -0.2);
            state.jump_delay = 9;
            let output = Simulator::default()
                .tick_with_controls(
                    &mut state,
                    MovementInput {
                        mode,
                        ..moving_frozen_input()
                    },
                    &Unqueried,
                )
                .unwrap();
            assert_eq!(state.tick, 43);
            assert_eq!(state.position, position);
            assert_eq!(state.velocity, Vec3::ZERO);
            assert_eq!(state.movement, Vec3::ZERO);
            assert_eq!(state.on_ground, on_ground);
            assert_eq!(
                state.jump_delay,
                if mode == MovementMode::Riding { 9 } else { 0 }
            );
            assert_eq!(output.tick_result.position, position);
            assert_eq!(output.tick_result.velocity, Vec3::ZERO);
            assert_eq!(output.tick_result.movement, Vec3::ZERO);
            assert!(!output.jump_initiated);
            assert!(output.controls.move_vector.iter().all(|&axis| axis != 0.0));
            assert!(output.tick_result.world_identity.chunks.is_empty());
            assert_eq!(
                output.tick_result.world_identity.registry,
                Unqueried.registry_identity()
            );
        }
    }
}

#[test]
fn releasing_immobility_does_not_restore_previous_velocity() {
    let simulator = Simulator::default();
    let mut state = PlayerState::new(Vec3::new(1.0, 10.0, 2.0));
    state.velocity = Vec3::new(0.4, 0.8, -0.6);
    simulator
        .tick(&mut state, moving_frozen_input(), &Unqueried)
        .unwrap();
    let mut stopped = PlayerState::new(state.position);
    stopped.tick = state.tick;
    let released = simulator
        .tick(&mut state, MovementInput::default(), &Empty)
        .unwrap();
    let expected = simulator
        .tick(&mut stopped, MovementInput::default(), &Empty)
        .unwrap();
    assert_eq!(released, expected);
    assert_eq!(state, stopped);
    assert_eq!(released.movement, Vec3::ZERO);
    assert_eq!(released.velocity.x, 0.0);
    assert_eq!(released.velocity.z, 0.0);
    assert!(released.velocity.y < 0.0);
}

#[test]
fn correction_replay_retains_immobile_ticks_and_discards_their_motion_overlays() {
    let simulator = Simulator::default();
    let mut state = PlayerState::new(Vec3::new(0.0, 10.0, 0.0));
    let mut history = PredictionHistory::new(8).unwrap();
    let inputs = [
        MovementInput {
            forward: 1.0,
            ..MovementInput::default()
        },
        moving_frozen_input(),
        moving_frozen_input(),
        MovementInput::default(),
    ];
    for input in inputs {
        history
            .predict(&mut state, input, &simulator, &Empty)
            .unwrap();
    }
    let mut corrected = history.state_at(1).unwrap().clone();
    corrected.position.x = 0.25;
    corrected.velocity = Vec3::new(0.9, -0.8, 0.7);
    let position = corrected.position;
    let (_, outputs) = history
        .rewind_and_replay_with_controls(
            &mut state,
            corrected,
            &simulator,
            &Empty,
            &[MotionOverlay {
                tick: 2,
                velocity: Vec3::new(-0.5, 0.4, 0.3),
            }],
        )
        .unwrap();
    for (index, tick) in outputs[..2].iter().enumerate() {
        assert_eq!(tick.tick_result.position, position);
        assert_eq!(tick.tick_result.velocity, Vec3::ZERO);
        assert_eq!(tick.tick_result.movement, Vec3::ZERO);
        assert_eq!(tick.controls.move_vector, [1.0, -1.0]);
        assert!(!tick.jump_initiated);
        assert_eq!(history.input_at(index as u64 + 2), Some(&inputs[index + 1]));
    }
    assert_eq!(outputs[2].tick_result.movement, Vec3::ZERO);
    assert_eq!(outputs[2].tick_result.velocity.x, 0.0);
    assert_eq!(outputs[2].tick_result.velocity.z, 0.0);
    assert!(!history.input_at(4).unwrap().immobile);
}

#[test]
fn immobile_tick_overflow_is_transactional() {
    let mut state = PlayerState::new(Vec3::new(0.0, 10.0, 0.0));
    state.tick = u64::MAX;
    state.velocity = Vec3::new(0.3, -0.8, 0.2);
    let original = state.clone();
    assert_eq!(
        Simulator::default().tick_with_controls(&mut state, moving_frozen_input(), &Unqueried),
        Err(SimulationError::TickOverflow)
    );
    assert_eq!(state, original);
}

#[test]
fn immobility_defaults_false_and_round_trips_when_captured() {
    let historical: MovementInput = serde_json::from_str(
        r#"{"strafe":0.0,"forward":0.0,"yaw_degrees":0.0,"jumping":false,"jump_pressed":false,"sprinting":false,"sneaking":false}"#,
    ).unwrap();
    assert!(!historical.immobile);
    assert!(
        serde_json::to_value(historical)
            .unwrap()
            .get("immobile")
            .is_none()
    );
    let captured = moving_frozen_input();
    let encoded = serde_json::to_value(captured).unwrap();
    assert_eq!(
        encoded.get("immobile"),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        serde_json::from_value::<MovementInput>(encoded).unwrap(),
        captured
    );
}
