use std::cell::Cell;

fn physics_after_one_second(frame_rate: u32) -> LocalPhysicsController {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);
    let mut elapsed = Duration::ZERO;
    for frame in 0..frame_rate {
        let delta = if frame + 1 == frame_rate {
            Duration::from_secs(1) - elapsed
        } else {
            Duration::from_secs_f64(1.0 / f64::from(frame_rate))
        };
        elapsed += delta;
        let result = physics.advance(delta, forward_physics_input(), &Floor);
        assert!(result.blocked.is_none());
    }
    physics
}

#[test]
fn local_physics_and_interpolation_are_equivalent_at_30_60_and_144_hz() {
    let at_30 = physics_after_one_second(30);
    let at_60 = physics_after_one_second(60);
    let at_144 = physics_after_one_second(144);

    assert_eq!(at_30.state(), at_60.state());
    assert_eq!(at_60.state(), at_144.state());
    assert_eq!(at_30.history_len(), 20);
    assert_eq!(at_60.history_len(), 20);
    assert_eq!(at_144.history_len(), 20);
    let eye_30 = at_30.render_eye_position().unwrap();
    let eye_60 = at_60.render_eye_position().unwrap();
    let eye_144 = at_144.render_eye_position().unwrap();
    for axis in 0..3 {
        assert!((eye_30[axis] - eye_60[axis]).abs() < 1.0e-5);
        assert!((eye_60[axis] - eye_144[axis]).abs() < 1.0e-5);
    }
}

#[test]
fn perspective_changes_leave_physics_history_and_outbox_unchanged() {
    let physics = physics_after_one_second(60);
    let expected_state = physics.state().unwrap().clone();
    let expected_history_len = physics.history_len();

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(completed_sample(101, [0.0, 2.620_01, -0.5]))
        .unwrap();
    ticker
        .enqueue_completed_physics(completed_sample(102, [0.0, 2.620_01, -1.0]))
        .unwrap();
    let expected_outbox = ticker.pending_snapshots();

    let mut camera = CameraSettingsAuthority::default();
    let mut settings = UserSettings::default();
    settings.gameplay.default_perspective = semantic_input::PerspectiveMode::ThirdPersonFront;
    camera.replace(1, &settings).unwrap();

    assert_eq!(physics.state(), Some(&expected_state));
    assert_eq!(physics.history_len(), expected_history_len);
    assert_eq!(ticker.pending_snapshots(), expected_outbox);
}

#[test]
fn correction_reanchors_feet_velocity_history_and_render_interpolation() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);
    physics.advance(Duration::from_millis(100), forward_physics_input(), &Floor);
    assert_eq!(physics.history_len(), 2);

    physics.reanchor_network_position([8.0, 71.620_01, 9.0], 150, false);

    let state = physics.state().expect("corrected physics state");
    assert_eq!(state.tick, 150);
    assert!((state.position.y - 70.0).abs() < 1.0e-5);
    assert_eq!(
        state.velocity,
        Vec3::ZERO,
        "CorrectPlayerMovePrediction.Delta is positional error, not velocity"
    );
    assert!(!state.on_ground);
    assert_eq!(physics.history_len(), 0);
    let eye = physics.render_eye_position().expect("corrected render eye");
    assert!((eye[1] - 71.62).abs() < 1.0e-5);
}

#[derive(Default)]
struct DeferredCollisionWorld {
    available: Cell<bool>,
}

impl DeferredCollisionWorld {
    fn set_available(&self) {
        self.available.set(true);
    }
}

impl CollisionWorld for DeferredCollisionWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        if !self.available.get() {
            return Err(WorldQueryError::UnloadedChunk(world::ChunkKey::new(0, 0, 0)));
        }
        Floor.collision_boxes(query)
    }
}

#[test]
fn unavailable_collision_defers_without_dropping_or_mutating_then_retries() {
    let world = DeferredCollisionWorld::default();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.0, 65.620_01, 6.0], 7, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 7, [4.0, 65.620_01, 6.0]);
    ticker.set_source(MovementSource::Physics);
    let before = physics.state().unwrap().clone();
    let before_eye = physics.render_eye_position();

    let frame = physics.advance(
        Duration::from_millis(250),
        forward_physics_input(),
        &world,
    );

    assert_eq!(frame.due_ticks, 5);
    assert_eq!(frame.completed_ticks, 0);
    assert_eq!(frame.dropped_ticks, 0);
    assert!(matches!(
        frame.blocked,
        Some(sim::SimulationError::World(
            WorldQueryError::UnloadedChunk(_)
        ))
    ));
    assert!(physics.is_active());
    assert!(ticker.physics_is_authorized());
    assert_eq!(physics_authority_fault_for_frame(&frame), None);
    assert_eq!(physics.state(), Some(&before));
    assert_eq!(physics.render_eye_position(), before_eye);
    assert_eq!(physics.history_len(), 0);

    world.set_available();
    let retry = physics.advance(Duration::ZERO, forward_physics_input(), &world);
    assert!(retry.blocked.is_none());
    assert_eq!(retry.completed_ticks, 5);
    assert_eq!(retry.dropped_ticks, 0);
    assert_eq!(physics.state().unwrap().tick, before.tick + 5);
    assert_eq!(physics.history_len(), 5);
}

#[test]
fn unknown_runtime_id_collision_data_is_also_deferred_as_transient() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.0, 65.620_01, 6.0], 7, true);

    let frame = physics.advance(
        Duration::from_millis(50),
        forward_physics_input(),
        &UnavailableWorld,
    );

    assert_eq!(frame.dropped_ticks, 0);
    assert!(matches!(
        frame.blocked,
        Some(sim::SimulationError::World(
            WorldQueryError::UnknownRuntimeId { runtime_id: 99, .. }
        ))
    ));
    assert!(physics.is_active());
    assert_eq!(physics_authority_fault_for_frame(&frame), None);
}

#[test]
fn local_physics_catch_up_overflow_remains_fatal_and_reports_due_ticks() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);

    let frame = physics.advance(Duration::from_secs(10), MovementInput::default(), &Floor);

    assert_eq!(frame.completed_ticks, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME);
    assert_eq!(frame.due_ticks, 200);
    assert_eq!(
        frame.dropped_ticks,
        200 - MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64
    );
    assert!(physics.history_len() <= 32);

    let fault = physics_authority_fault_for_frame(&frame).expect("catch-up overflow fault");
    assert_eq!(
        fault,
        PhysicsAuthorityFault::PhysicsTickOverflow {
            due: 200,
            dropped: 200 - MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64,
        }
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 0, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker.record_physics_fault(fault);
    assert!(!ticker.physics_is_authorized());
    assert!(matches!(
        ticker.take_authority_fault().unwrap().fault,
        PhysicsAuthorityFault::PhysicsTickOverflow {
            due: 200,
            dropped: 192
        }
    ));
}

#[test]
fn non_retryable_simulation_errors_fail_closed_with_distinct_diagnostics() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);
    let invalid_input = MovementInput {
        forward: f64::NAN,
        ..MovementInput::default()
    };

    let frame = physics.advance(Duration::from_millis(50), invalid_input, &Floor);
    assert_eq!(frame.due_ticks, 1);
    assert_eq!(frame.dropped_ticks, 0);
    assert!(matches!(
        frame.blocked,
        Some(sim::SimulationError::NonFiniteInput { field: "forward" })
    ));

    let fault = physics_authority_fault_for_frame(&frame).expect("simulation error fault");
    assert_eq!(
        fault,
        PhysicsAuthorityFault::PhysicsSimulationError {
            due: 1,
            tick_index: 0,
            error: sim::SimulationError::NonFiniteInput { field: "forward" },
        }
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 0, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker.record_physics_fault(fault);
    assert!(!ticker.physics_is_authorized());
    assert!(matches!(
        ticker.take_authority_fault().unwrap().fault,
        PhysicsAuthorityFault::PhysicsSimulationError {
            due: 1,
            tick_index: 0,
            error: sim::SimulationError::NonFiniteInput { field: "forward" }
        }
    ));
}

#[test]
fn frame_boundary_reanchor_discards_only_pre_anchor_elapsed() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position_before_advance([0.0, 2.620_01, 0.0], 0, true);

    let startup = physics.advance(Duration::from_secs(10), MovementInput::default(), &Floor);
    assert_eq!(startup.completed_ticks, 0);
    assert_eq!(startup.dropped_ticks, 0);
    assert!(startup.samples.is_empty());
    assert_eq!(physics.state().expect("anchored state").tick, 0);

    let overloaded = physics.advance(Duration::from_secs(10), MovementInput::default(), &Floor);
    assert_eq!(
        overloaded.completed_ticks,
        MAX_LOCAL_PHYSICS_TICKS_PER_FRAME
    );
    assert_eq!(
        overloaded.dropped_ticks,
        200 - MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64
    );
}

fn synthetic_preg(breg: &[u8], records: &[RegistryRecord]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"PREG1001");
    bytes.extend_from_slice(&1001_u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(records.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&Sha256::digest(breg));
    for record in records {
        bytes.extend_from_slice(&record.sequential_id.to_le_bytes());
        bytes.extend_from_slice(&record.network_hash.to_le_bytes());
        bytes.push(u8::try_from(record.collision_seed.boxes.len()).unwrap());
        bytes.push(if record.collision_seed.boxes.is_empty() {
            BlockPhysicsFlags::PASSABLE.bits()
        } else {
            0
        });
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&60_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&100_000_000_u32.to_le_bytes());
        bytes.extend_from_slice(&0_i32.to_le_bytes());
        for shape in &record.collision_seed.boxes {
            for coordinate in [
                shape.min_x,
                shape.min_y,
                shape.min_z,
                shape.max_x,
                shape.max_y,
                shape.max_z,
            ] {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
    }
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    bytes
}

#[test]
fn checked_in_registry_registers_every_preg_fact_in_both_id_modes() {
    let breg = include_bytes!("../../../../crates/assets/data/block-registry-v1001.bin");
    let records = read_registry(breg).expect("checked-in BREG1001");
    let preg = synthetic_preg(breg, &records);

    let registries = PhysicsCollisionRegistries::from_assets(breg, &records, &preg)
        .expect("BREG-bound PREG facts are valid");

    assert_eq!(
        registries.registered_count(NetworkIdMode::Sequential),
        records.len()
    );
    assert_eq!(
        registries.registered_count(NetworkIdMode::Hashed),
        records.len()
    );
    assert_eq!(registries.available_record_count(), records.len());
    assert_eq!(
        registries
            .registry(NetworkIdMode::Sequential)
            .identity()
            .preg_sha256,
        Sha256::digest(&preg).as_slice()
    );
    assert_ne!(
        registries
            .registry(NetworkIdMode::Sequential)
            .identity()
            .id_space,
        registries
            .registry(NetworkIdMode::Hashed)
            .identity()
            .id_space,
    );
}

#[test]
fn app_axes_map_to_bedsim_strafe_forward_and_clear_when_input_is_inactive() {
    let active = physics_movement_input([1.0, 1.0], 180.0, true, true, true, true);
    assert_eq!(active.strafe, -1.0, "D is bedsim's negative strafe");
    assert_eq!(active.forward, 1.0);
    assert_eq!(active.yaw_degrees, 180.0);
    assert!(active.jumping);
    assert!(active.sneaking);
    assert!(active.sprinting);

    assert_eq!(
        physics_movement_input([1.0, 1.0], 90.0, false, true, true, true),
        MovementInput::default()
    );
}

#[test]
fn jump_edge_is_latched_across_render_frames_until_the_next_fixed_tick() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);
    let jumping = MovementInput {
        jumping: true,
        ..MovementInput::default()
    };

    assert_eq!(
        physics
            .advance(Duration::from_secs_f64(1.0 / 60.0), jumping, &Floor)
            .completed_ticks,
        0
    );
    assert_eq!(
        physics
            .advance(Duration::from_secs_f64(1.0 / 60.0), jumping, &Floor)
            .completed_ticks,
        0
    );
    assert_eq!(
        physics
            .advance(Duration::from_secs_f64(1.0 / 60.0), jumping, &Floor)
            .completed_ticks,
        1
    );

    assert!(physics.state().unwrap().position.y > 1.0);
}

#[test]
fn holding_jump_repeats_only_after_the_player_lands() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);
    let jumping = MovementInput {
        jumping: true,
        ..MovementInput::default()
    };
    let mut takeoffs = 0;
    let mut was_grounded = true;

    for _ in 0..80 {
        let frame = physics.advance(Duration::from_millis(50), jumping, &Floor);
        assert!(frame.blocked.is_none());
        let grounded = physics.state().unwrap().on_ground;
        if was_grounded && !grounded {
            takeoffs += 1;
        }
        was_grounded = grounded;
    }

    assert!(
        takeoffs >= 2,
        "a continuously held jump should take off again after landing"
    );
}
