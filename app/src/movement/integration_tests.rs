use std::time::Duration;

use super::{
    LocalPhysicsController, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME, MovementInputSample,
    MovementSendError, MovementSource, MovementTicker, OUTBOX_CAPACITY, PhysicsCollisionRegistries,
    flush_player_auth_inputs, physics_movement_input,
};
use assets::{CollisionConfidence, NetworkIdMode, read_registry};
use protocol::PlayerInputFlags;
use sim::{Aabb, CollisionWorld, MovementInput, Vec3, WorldQueryError};

fn sample(position: [f32; 3]) -> MovementInputSample {
    MovementInputSample {
        position,
        move_vector: [0.0, 1.0],
        pitch: 10.0,
        yaw: 20.0,
        head_yaw: 20.0,
        camera_orientation: [0.0, 0.0, 1.0],
        jumping: false,
        sneaking: false,
        sprinting: false,
    }
}

#[test]
fn default_free_camera_never_enqueues_or_sends_after_start_game_and_correction() {
    let mut ticker = MovementTicker::default();

    // StartGame initializes session/tick state, but the app is still using
    // its independent fly camera rather than physics-authoritative movement.
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.advance(
        MovementSource::FreeCamera,
        Duration::from_millis(50),
        sample([100.0, 200.0, 300.0]),
    );

    // A server correction may reanchor local state, but must not authorize
    // the free camera as an outbound movement source.
    ticker.apply_server_correction(1_050, [8.0, 70.0, 9.0]);
    ticker.advance(
        MovementSource::FreeCamera,
        Duration::from_millis(50),
        sample([400.0, 500.0, 600.0]),
    );

    let mut sent_packets = 0;
    let flushed = flush_player_auth_inputs(&mut ticker, 8, |_packet| {
        sent_packets += 1;
        Ok::<_, &str>(())
    })
    .unwrap();

    assert_eq!(flushed, 0);
    assert_eq!(sent_packets, 0);
    assert_eq!(ticker.pending_count(), 0);
}

#[test]
fn physics_authority_can_use_the_existing_twenty_hertz_scheduler() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);

    for _ in 0..60 {
        ticker.advance(
            MovementSource::Physics,
            Duration::from_secs_f64(1.0 / 60.0),
            sample([1.0, 64.0, 2.0]),
        );
    }

    let mut sent_packets = 0;
    let flushed = flush_player_auth_inputs(&mut ticker, usize::MAX, |_packet| {
        sent_packets += 1;
        Ok::<_, &str>(())
    })
    .unwrap();

    assert_eq!(flushed, 20);
    assert_eq!(sent_packets, 20);
}

#[test]
fn start_game_free_camera_reset_discards_queued_physics_and_stays_suppressed() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker.advance(
        MovementSource::Physics,
        Duration::from_millis(50),
        sample([1.0, 2.0, 3.0]),
    );
    assert_eq!(ticker.pending_count(), 1);

    // A replacement StartGame explicitly restores the app's current source.
    ticker.set_source(MovementSource::FreeCamera);
    assert_eq!(ticker.pending_count(), 0);
    ticker.reset(2, 1_000, [8.0, 70.0, 9.0]);
    ticker.apply_server_correction(1_050, [10.0, 72.0, 11.0]);
    ticker.advance(
        MovementSource::FreeCamera,
        Duration::from_millis(50),
        sample([400.0, 500.0, 600.0]),
    );

    let mut sent_packets = 0;
    let flushed = flush_player_auth_inputs(&mut ticker, 8, |_packet| {
        sent_packets += 1;
        Ok::<_, &str>(())
    })
    .unwrap();

    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(flushed, 0);
    assert_eq!(sent_packets, 0);
}

#[test]
fn physics_authority_rejects_a_free_camera_sample_origin() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);

    ticker.advance(
        MovementSource::FreeCamera,
        Duration::from_millis(50),
        sample([100.0, 200.0, 300.0]),
    );

    let mut sent_packets = 0;
    let flushed = flush_player_auth_inputs(&mut ticker, 8, |_packet| {
        sent_packets += 1;
        Ok::<_, &str>(())
    })
    .unwrap();

    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(flushed, 0);
    assert_eq!(sent_packets, 0);
}

#[test]
fn free_camera_authority_rejects_retry_enqueue() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    ticker.advance(
        MovementSource::Physics,
        Duration::from_millis(50),
        sample([2.0, 64.0, 3.0]),
    );
    let snapshot = ticker.pop_pending().unwrap();

    ticker.set_source(MovementSource::FreeCamera);
    assert_eq!(ticker.retry_front(snapshot), Err(snapshot));
    assert_eq!(ticker.pending_count(), 0);
}

#[test]
fn deterministic_accumulator_emits_exactly_twenty_ticks_per_second() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);

    for frame in 0..60 {
        ticker.advance(
            MovementSource::Physics,
            Duration::from_secs_f64(1.0 / 60.0),
            sample([1.0, 64.0, 2.0]),
        );
        if frame < 2 {
            assert_eq!(ticker.pending_count(), 0);
        }
    }

    let snapshots: Vec<_> = std::iter::from_fn(|| ticker.pop_pending()).collect();
    assert_eq!(snapshots.len(), 20);
    assert_eq!(snapshots.first().unwrap().tick, 1_001);
    assert_eq!(snapshots.last().unwrap().tick, 1_020);
}

#[test]
fn tick_snapshots_encode_held_and_edge_flags_and_position_delta() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 41, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    let mut pressed = sample([1.25, 64.0, 1.5]);
    pressed.jumping = true;
    pressed.sprinting = true;

    ticker.advance(MovementSource::Physics, Duration::from_millis(50), pressed);
    let first = ticker.pop_pending().unwrap();
    assert_eq!(first.tick, 42);
    assert_eq!(first.delta, [0.25, 0.0, -0.5]);
    assert_eq!(first.move_vector, [0.0, 1.0]);
    assert_eq!(first.position, pressed.position);
    assert_ne!(first.flags.bits() & PlayerInputFlags::UP.bits(), 0);
    assert_ne!(first.flags.bits() & PlayerInputFlags::JUMPING.bits(), 0);
    assert_ne!(
        first.flags.bits() & PlayerInputFlags::START_JUMPING.bits(),
        0
    );
    assert_ne!(
        first.flags.bits() & PlayerInputFlags::JUMP_PRESSED_RAW.bits(),
        0
    );
    assert_ne!(first.flags.bits() & PlayerInputFlags::SPRINTING.bits(), 0);
    assert_ne!(
        first.flags.bits() & PlayerInputFlags::START_SPRINTING.bits(),
        0
    );

    ticker.advance(MovementSource::Physics, Duration::from_millis(50), pressed);
    let held = ticker.pop_pending().unwrap();
    assert_eq!(held.tick, 43);
    assert_eq!(held.delta, [0.0; 3]);
    assert_eq!(
        held.flags.bits() & PlayerInputFlags::START_JUMPING.bits(),
        0
    );
    assert_eq!(
        held.flags.bits() & PlayerInputFlags::START_SPRINTING.bits(),
        0
    );
    assert_ne!(
        held.flags.bits() & PlayerInputFlags::JUMP_CURRENT_RAW.bits(),
        0
    );

    let released = sample(pressed.position);
    ticker.advance(MovementSource::Physics, Duration::from_millis(50), released);
    let released = ticker.pop_pending().unwrap();
    assert_ne!(
        released.flags.bits() & PlayerInputFlags::JUMP_RELEASED_RAW.bits(),
        0
    );
    assert_ne!(
        released.flags.bits() & PlayerInputFlags::STOP_SPRINTING.bits(),
        0
    );
}

#[test]
fn overdue_ticks_distribute_the_frame_position_delta_evenly() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 100, [0.0, 64.0, 0.0]);
    ticker.set_source(MovementSource::Physics);

    ticker.advance(
        MovementSource::Physics,
        Duration::from_millis(150),
        sample([3.0, 64.0, -1.5]),
    );

    let snapshots: Vec<_> = std::iter::from_fn(|| ticker.pop_pending()).collect();
    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[0].position, [1.0, 64.0, -0.5]);
    assert_eq!(snapshots[1].position, [2.0, 64.0, -1.0]);
    assert_eq!(snapshots[2].position, [3.0, 64.0, -1.5]);
    assert_eq!(snapshots[0].delta, [1.0, 0.0, -0.5]);
    assert_eq!(snapshots[1].delta, [1.0, 0.0, -0.5]);
    assert_eq!(snapshots[2].delta, [1.0, 0.0, -0.5]);
}

#[test]
fn outbox_is_bounded_and_session_reset_discards_stale_ticks_and_input_edges() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker.advance(
        MovementSource::Physics,
        Duration::from_secs(2),
        sample([2.0, 3.0, 4.0]),
    );
    assert_eq!(ticker.pending_count(), OUTBOX_CAPACITY);
    assert_eq!(ticker.dropped_tick_count(), 40 - OUTBOX_CAPACITY as u64);

    let retry = ticker.pop_pending().unwrap();
    ticker.retry_front(retry).unwrap();
    assert_eq!(ticker.pending_count(), OUTBOX_CAPACITY);

    ticker.reset(2, 5_000, [9.0, 10.0, 11.0]);
    assert_eq!(ticker.session_generation(), 2);
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(ticker.dropped_tick_count(), 0);
    ticker.advance(
        MovementSource::Physics,
        Duration::from_millis(50),
        sample([9.0, 10.0, 11.0]),
    );
    let new_session = ticker.pop_pending().unwrap();
    assert_eq!(new_session.tick, 5_001);
    assert_eq!(new_session.delta, [0.0; 3]);
    assert_eq!(
        new_session.flags.bits() & PlayerInputFlags::START_JUMPING.bits(),
        0
    );
    ticker.deactivate();
    ticker.advance(
        MovementSource::Physics,
        Duration::from_millis(50),
        sample([9.0, 10.0, 11.0]),
    );
    assert_eq!(ticker.pending_count(), 0);
}

#[test]
fn retry_front_rejects_over_capacity_without_losing_the_snapshot() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 0, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker.advance(
        MovementSource::Physics,
        Duration::from_millis(50 * OUTBOX_CAPACITY as u64),
        sample([0.0; 3]),
    );
    let snapshot = ticker.pop_pending().unwrap();
    ticker.retry_front(snapshot).unwrap();
    let duplicate = *ticker.peek_pending().unwrap();
    let error = ticker.retry_front(duplicate).unwrap_err();
    assert_eq!(error, duplicate);
    assert_eq!(ticker.pending_count(), OUTBOX_CAPACITY);
}

#[test]
fn server_correction_reanchors_delta_and_discards_unacknowledged_prediction_ticks() {
    let mut ticker = MovementTicker::default();
    ticker.reset(3, 100, [1.0, 64.0, 1.0]);
    ticker.set_source(MovementSource::Physics);
    ticker.advance(
        MovementSource::Physics,
        Duration::from_millis(100),
        sample([2.0, 64.0, 2.0]),
    );
    assert_eq!(ticker.pending_count(), 2);

    ticker.apply_server_correction(150, [8.0, 70.0, 9.0]);
    assert_eq!(ticker.pending_count(), 0);
    ticker.advance(
        MovementSource::Physics,
        Duration::from_millis(50),
        sample([8.0, 70.0, 9.0]),
    );
    let corrected = ticker.pop_pending().unwrap();
    assert_eq!(corrected.tick, 151);
    assert_eq!(corrected.delta, [0.0; 3]);
}

#[test]
fn bounded_flush_restores_the_exact_front_snapshot_when_transport_is_full() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker.advance(
        MovementSource::Physics,
        Duration::from_millis(100),
        sample([1.0, 2.0, 3.0]),
    );
    let expected = *ticker.peek_pending().unwrap();

    let error = flush_player_auth_inputs(&mut ticker, 8, |_packet| Err("full")).unwrap_err();
    assert!(matches!(error, MovementSendError::Transport("full")));
    assert_eq!(ticker.pending_count(), 2);
    assert_eq!(*ticker.peek_pending().unwrap(), expected);

    let mut sent_packets = 0;
    let sent = flush_player_auth_inputs(&mut ticker, 8, |_packet| {
        sent_packets += 1;
        Ok::<_, &str>(())
    })
    .unwrap();
    assert_eq!(sent, 2);
    assert_eq!(sent_packets, 2);
    assert_eq!(ticker.pending_count(), 0);
}

#[test]
fn keyboard_diagonal_is_normalized_without_losing_the_raw_vector() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 0, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    let mut diagonal = sample([0.0; 3]);
    diagonal.move_vector = [1.0, 1.0];
    ticker.advance(MovementSource::Physics, Duration::from_millis(50), diagonal);
    let snapshot = ticker.pop_pending().unwrap();

    let component = 1.0_f32 / 2.0_f32.sqrt();
    assert!((snapshot.move_vector[0] - component).abs() < 1e-6);
    assert!((snapshot.move_vector[1] - component).abs() < 1e-6);
    assert_eq!(snapshot.raw_move_vector, [1.0, 1.0]);
    assert_eq!(snapshot.analogue_move_vector, snapshot.move_vector);
}

struct Floor;

impl CollisionWorld for Floor {
    fn collision_boxes(&self, query: Aabb) -> Result<Vec<Aabb>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(64.0, 1.0, 64.0));
        Ok(floor
            .intersects(query)
            .then_some(floor)
            .into_iter()
            .collect())
    }
}

struct UnavailableWorld;

impl CollisionWorld for UnavailableWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<Vec<Aabb>, WorldQueryError> {
        Err(WorldQueryError::UnknownRuntimeId {
            runtime_id: 99,
            block: [0, 0, 0],
        })
    }
}

fn forward_physics_input() -> MovementInput {
    MovementInput {
        forward: 1.0,
        yaw_degrees: 180.0,
        ..MovementInput::default()
    }
}

#[test]
fn local_physics_runs_exactly_twenty_fixed_ticks_and_interpolates_the_eye() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);

    for _ in 0..60 {
        let frame = physics.advance(
            Duration::from_secs_f64(1.0 / 60.0),
            forward_physics_input(),
            &Floor,
        );
        assert!(frame.blocked.is_none());
    }

    let state = physics.state().expect("physics is anchored");
    assert_eq!(state.tick, 20);
    assert!(
        state.position.z < 0.0,
        "forward at yaw 180 faces negative Z"
    );
    assert_eq!(physics.history_len(), 20);
    let eye = physics.render_eye_position().expect("interpolated eye");
    assert!(eye.iter().all(|component| component.is_finite()));
    assert!(eye[2] <= 0.0);
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

#[test]
fn unavailable_collision_fails_closed_without_advancing_or_drifting_the_camera() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.0, 65.620_01, 6.0], 7, true);
    let before = physics.state().unwrap().clone();
    let before_eye = physics.render_eye_position();

    let frame = physics.advance(
        Duration::from_millis(50),
        forward_physics_input(),
        &UnavailableWorld,
    );

    assert!(matches!(
        frame.blocked,
        Some(sim::SimulationError::World(
            WorldQueryError::UnknownRuntimeId { runtime_id: 99, .. }
        ))
    ));
    assert_eq!(physics.state(), Some(&before));
    assert_eq!(physics.render_eye_position(), before_eye);
    assert_eq!(physics.history_len(), 0);
}

#[test]
fn local_physics_catch_up_and_prediction_history_are_bounded() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 0, true);

    let frame = physics.advance(Duration::from_secs(10), MovementInput::default(), &Floor);

    assert_eq!(frame.completed_ticks, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME);
    assert_eq!(
        frame.dropped_ticks,
        200 - MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64
    );
    assert!(physics.history_len() <= 32);
}

#[test]
fn checked_in_registry_registers_every_available_collision_in_both_id_modes() {
    let records = read_registry(include_bytes!(
        "../../../crates/assets/data/block-registry-v1001.bin"
    ))
    .expect("checked-in BREG1003");
    let available = records
        .iter()
        .filter(|record| record.collision_seed.confidence != CollisionConfidence::None)
        .count();

    let registries = PhysicsCollisionRegistries::from_records(&records)
        .expect("checked-in collision seeds are valid");

    assert_eq!(
        registries.registered_count(NetworkIdMode::Sequential),
        available
    );
    assert_eq!(
        registries.registered_count(NetworkIdMode::Hashed),
        available
    );
    assert_eq!(registries.available_record_count(), available);
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
