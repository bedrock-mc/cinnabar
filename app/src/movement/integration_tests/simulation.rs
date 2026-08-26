use std::cell::Cell;

use crate::runtime::phase3_evidence::Phase3EvidenceEmitter;

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

fn run_one_tick(physics: &mut LocalPhysicsController, world: &VersionedFloor) -> PhysicsMovementSample {
    let frame = physics.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        world,
    );
    assert!(
        frame.blocked.is_none(),
        "unexpected blocked tick: {:?}",
        frame.blocked
    );
    assert_eq!(frame.samples.len(), 1);
    frame.samples.into_iter().next().unwrap()
}

#[test]
fn queued_server_motion_replaces_exactly_one_ticks_velocity() {
    let mut walking = LocalPhysicsController::default();
    walking.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let plain = run_one_tick(&mut walking, &VersionedFloor(1));

    let mut knocked = LocalPhysicsController::default();
    knocked.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    knocked.queue_server_motion([0.45, 0.42, -0.35], 101);
    let hit = run_one_tick(&mut knocked, &VersionedFloor(1));

    assert_eq!(hit.tick, plain.tick);
    assert!(
        hit.position[0] > plain.position[0] + 0.2,
        "knockback must dominate the first post-hit tick: {:?} vs {:?}",
        hit.position,
        plain.position
    );
    assert!(
        hit.position[1] > plain.position[1],
        "upward knockback must lift the arc"
    );
    assert!(hit.velocity[2] < plain.velocity[2]);

    // The impulse is one-shot: the next tick shows gravity resuming and the
    // arc continuing, not a fresh upward application.
    let resumed = run_one_tick(&mut knocked, &VersionedFloor(1));
    let resumed_plain = run_one_tick(&mut walking, &VersionedFloor(1));
    assert!(
        resumed.velocity[1] < hit.velocity[1],
        "the upward impulse must not refire: {:?} then {:?}",
        hit.velocity,
        resumed.velocity
    );
    assert!(
        resumed.velocity[2] < resumed_plain.velocity[2],
        "lateral knockback momentum carries into the following tick"
    );
}

#[test]
fn authoritative_server_motion_ticks_preserve_ordered_future_impulses() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    physics.queue_server_motion([0.45, 0.42, -0.35], 102);
    physics.queue_server_motion([0.6, 0.5, -0.1], 103);
    physics.queue_server_motion([-0.2, 0.8, 0.3], 103);

    let tick_101 = run_one_tick(&mut physics, &VersionedFloor(1));
    let tick_102 = run_one_tick(&mut physics, &VersionedFloor(1));
    let tick_103 = run_one_tick(&mut physics, &VersionedFloor(1));

    assert_eq!([tick_101.tick, tick_102.tick, tick_103.tick], [101, 102, 103]);
    assert!(tick_102.velocity[0] > 0.0, "the first impulse keeps its wire tick");
    assert!(tick_103.velocity[0] < 0.0, "the second impulse is not coalesced into the first");
}

#[test]
fn non_finite_server_motion_is_ignored_and_inactive_controllers_drop_it() {
    let mut physics = LocalPhysicsController::default();
    physics.queue_server_motion([f32::NAN, 0.0, 0.0], 1);
    physics.queue_server_motion([0.0, f32::INFINITY, 0.0], 1);
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);

    let mut baseline = LocalPhysicsController::default();
    baseline.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);

    let hit = run_one_tick(&mut physics, &VersionedFloor(1));
    let plain = run_one_tick(&mut baseline, &VersionedFloor(1));
    assert_eq!(hit.position, plain.position);
}

#[test]
fn correction_replay_reapplies_retained_server_motion_overlays() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let t101 = run_one_tick(&mut physics, &VersionedFloor(1));
    physics.queue_server_motion([0.45, 0.42, -0.35], 102);
    let t102 = run_one_tick(&mut physics, &VersionedFloor(1));
    let t103 = run_one_tick(&mut physics, &VersionedFloor(1));
    assert_eq!(t102.tick, 102);
    assert!(t102.position[0] > t101.position[0] + 0.2);

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in [t101.clone(), t102.clone(), t103.clone()] {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let sent = ticker.pop_pending().unwrap();
    assert_eq!(sent.snapshot.tick, 101);

    // The server confirms tick 101 exactly where the client predicted it, so
    // the replay's only job is to re-run 102..103 from that anchor.
    let outcome = reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        t101.position,
        101,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();
    assert!(matches!(
        outcome,
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: 101,
            replayed_ticks: 2,
        }
    ));

    let replayed: Vec<_> = ticker
        .pending_samples()
        .iter()
        .map(|pending| pending.snapshot.position)
        .collect();
    assert_eq!(replayed.as_slice(), &[t102.position, t103.position]);
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
fn multi_tick_frames_snapshot_the_authority_present_at_the_frame_boundary() {
    let mut batched = LocalPhysicsController::default();
    let mut split = LocalPhysicsController::default();
    for physics in [&mut batched, &mut split] {
        physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);
    }
    let slower = MovementInput {
        forward: 1.0,
        movement_speed: Some(0.2),
        ..MovementInput::default()
    };
    let faster = MovementInput {
        forward: 1.0,
        movement_speed: Some(0.4),
        ..MovementInput::default()
    };

    let frame = batched.advance(Duration::from_millis(100), slower, &Floor);
    assert_eq!(frame.completed_ticks, 2);
    split.advance(Duration::from_millis(50), slower, &Floor);
    split.advance(Duration::from_millis(50), slower, &Floor);
    assert_eq!(batched.state(), split.state());

    batched.advance(Duration::from_millis(50), faster, &Floor);
    split.advance(Duration::from_millis(50), faster, &Floor);
    assert_eq!(batched.state(), split.state());
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
fn unavailable_collision_discards_blocked_elapsed_without_mutating_then_retries_fresh() {
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
    assert_eq!(retry.completed_ticks, 0);
    assert_eq!(retry.due_ticks, 0);
    assert_eq!(retry.dropped_ticks, 0);
    assert_eq!(physics.state(), Some(&before));
    assert_eq!(physics.history_len(), 0);

    let resumed = physics.advance(Duration::from_millis(50), forward_physics_input(), &world);
    assert!(resumed.blocked.is_none());
    assert_eq!(resumed.completed_ticks, 1);
    assert_eq!(resumed.dropped_ticks, 0);
    assert_eq!(physics.state().unwrap().tick, before.tick + 1);
    assert_eq!(physics.history_len(), 1);
}

#[test]
fn unavailable_collision_does_not_overflow_after_a_long_block() {
    let world = DeferredCollisionWorld::default();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.0, 65.620_01, 6.0], 7, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 7, [4.0, 65.620_01, 6.0]);
    ticker.set_source(MovementSource::Physics);
    let mut evidence = Phase3EvidenceEmitter::default();
    let before = physics.state().unwrap().clone();
    let before_eye = physics.render_eye_position();
    let mut violation_markers = Vec::new();

    let frame = physics.advance(
        Duration::from_millis(600),
        forward_physics_input(),
        &world,
    );
    assert_eq!(frame.due_ticks, 12);
    assert_eq!(frame.completed_ticks, 0);
    assert_eq!(frame.dropped_ticks, 0);
    assert!(frame.blocked.is_some());
    if let Some(fault) = physics_authority_fault_for_frame(&frame) {
        ticker.record_physics_fault(fault);
        if let Some(record) = ticker.take_authority_fault() {
            violation_markers.extend(evidence.observe_authority_fault(record));
        }
    }

    assert!(ticker.physics_is_authorized());
    assert!(ticker.take_authority_fault().is_none());
    assert!(violation_markers.is_empty());
    assert!(evidence.take_violation_marker().is_empty());
    assert_eq!(physics.state(), Some(&before));
    assert_eq!(physics.render_eye_position(), before_eye);
    assert_eq!(physics.history_len(), 0);
    assert_eq!(physics.dropped_tick_count(), 0);
}

#[test]
fn collision_data_returning_after_a_long_block_does_not_immediately_fault() {
    let world = DeferredCollisionWorld::default();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.0, 65.620_01, 6.0], 7, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 7, [4.0, 65.620_01, 6.0]);
    ticker.set_source(MovementSource::Physics);

    for _ in 0..12 {
        let frame = physics.advance(
            Duration::from_millis(50),
            forward_physics_input(),
            &world,
        );
        assert!(frame.blocked.is_some());
        assert_eq!(frame.dropped_ticks, 0);
        assert_eq!(physics_authority_fault_for_frame(&frame), None);
    }

    world.set_available();
    let frame = physics.advance(
        Duration::from_millis(50),
        forward_physics_input(),
        &world,
    );
    assert!(frame.blocked.is_none());
    assert_eq!(frame.completed_ticks, 1);
    assert_eq!(frame.dropped_ticks, 0);
    assert_eq!(physics_authority_fault_for_frame(&frame), None);
    assert!(ticker.physics_is_authorized());
    assert_eq!(physics.state().unwrap().tick, 8);
    assert_eq!(physics.history_len(), 1);
}

#[test]
fn unknown_runtime_id_collision_data_is_also_deferred_as_transient() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.0, 65.620_01, 6.0], 7, true);
    let before = physics.state().unwrap().clone();
    let before_eye = physics.render_eye_position();

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
    assert_eq!(physics.state(), Some(&before));
    assert_eq!(physics.render_eye_position(), before_eye);
    assert_eq!(physics.history_len(), 0);
}

#[test]
fn local_physics_catch_up_overflow_stays_contiguous_and_keeps_authority() {
    // A render-frame stall that exceeds the per-frame tick budget drops the
    // excess due time instead of simulating it late. The retained samples stay
    // contiguous and monotonic, so the outbound input stream remains a valid
    // 20 Hz sequence and time starvation alone must not revoke authority.
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
    assert_eq!(
        physics_authority_fault_for_frame(&frame),
        None,
        "time starvation is not an authority fault"
    );

    let mut ticker = MovementTicker::default();
    ticker.reset(1, 0, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    let mut ticks = Vec::new();
    for sample in &frame.samples {
        ticker.enqueue_completed_physics(sample.clone()).unwrap();
        ticks.push(sample.tick);
    }
    assert_eq!(ticks, (1..=MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64).collect::<Vec<_>>());
    assert!(ticker.physics_is_authorized());
    assert!(ticker.take_authority_fault().is_none());
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

pub(super) fn synthetic_preg(breg: &[u8], records: &[RegistryRecord]) -> Vec<u8> {
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
    let active = physics_movement_input([1.0, 1.0], 180.0, true, true, true, true, true);
    assert_eq!(active.strafe, -1.0, "D is bedsim's negative strafe");
    assert_eq!(active.forward, 1.0);
    assert_eq!(active.yaw_degrees, 180.0);
    assert!(active.jumping);
    assert!(active.sneaking);
    assert!(active.sprinting);
    assert!(
        !active.using_consumable,
        "generic Use is not evidence that the held item is consumable"
    );

    assert_eq!(
        physics_movement_input([1.0, 1.0], 90.0, false, true, true, true, true),
        MovementInput::default()
    );
}

#[test]
fn processed_sprint_requires_forward_movement_input() {
    // Vanilla sprints only while moving forward. A held sprint request with
    // backward, strafe-only, or no movement input is not an active sprint:
    // neither the simulator speed nor the outbound flags may claim one.
    let backward = physics_movement_input([0.0, -1.0], 180.0, true, false, false, true, false);
    assert!(!backward.sprinting, "backward input cannot sprint");
    let strafe_only = physics_movement_input([1.0, 0.0], 180.0, true, false, false, true, false);
    assert!(!strafe_only.sprinting, "strafe-only input cannot sprint");
    let stationary = physics_movement_input([0.0, 0.0], 180.0, true, false, false, true, false);
    assert!(!stationary.sprinting, "stationary input cannot sprint");

    let forward = physics_movement_input([0.0, 1.0], 180.0, true, false, false, true, false);
    assert!(forward.sprinting);
    assert_eq!(forward.forward, 1.0);

    // Sneaking does not cancel an active sprint: vanilla keeps the faster
    // sneak-sprint pace, so the forward gate alone decides processed sprint.
    let sneaking_forward =
        physics_movement_input([0.0, 1.0], 180.0, true, false, true, true, false);
    assert!(sneaking_forward.sprinting);
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
