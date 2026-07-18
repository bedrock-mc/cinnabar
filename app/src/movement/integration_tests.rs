use std::time::Duration;

use super::{
    LocalPhysicsController, MAX_LOCAL_PHYSICS_TICKS_PER_FRAME, MovementOutboxReconciliation,
    MovementSendError, MovementSource, MovementTicker, OUTBOX_CAPACITY, PhysicsAuthorityFault,
    PhysicsAuthorityGate, PhysicsCollisionRegistries, PhysicsCorrectionMode,
    PhysicsCorrectionOutcome, PhysicsMovementSample, PhysicsSampleContext,
    flush_player_auth_inputs, physics_movement_input, reconcile_candidate_physics_correction,
};
use assets::{BlockPhysicsFlags, NetworkIdMode, RegistryRecord, read_registry};
use protocol::{PlayerInputFlags, PlayerInputMode};
use sha2::{Digest, Sha256};
use sim::{
    Aabb, CollisionIdSpace, CollisionQuery, CollisionRegistryIdentity, CollisionWorld,
    MovementInput, Vec3, WorldCollisionIdentity, WorldQueryError,
};
use ui::UserSettings;

use crate::camera::CameraSettingsAuthority;

fn fixture_world_identity(seed: u8) -> WorldCollisionIdentity {
    WorldCollisionIdentity::new(
        CollisionRegistryIdentity {
            protocol: 1001,
            id_space: CollisionIdSpace::Sequential,
            preg_sha256: [seed; 32],
        },
        [],
    )
    .unwrap()
}

fn completed_sample(tick: u64, position: [f32; 3]) -> PhysicsMovementSample {
    PhysicsMovementSample {
        tick,
        position,
        move_vector: [0.0, 1.0],
        pitch: 10.0,
        yaw: 20.0,
        head_yaw: 20.0,
        camera_orientation: [0.0, 0.0, 1.0],
        jumping: false,
        sneaking: false,
        sprinting: false,
        input_mode: PlayerInputMode::Mouse,
        grounded_before_tick: false,
        grounded_after_tick: false,
        jump_repeated: false,
        world_identity: fixture_world_identity(1),
    }
}

#[test]
fn candidate_physics_authority_is_explicit_complete_and_auto_fly_safe() {
    assert_eq!(
        PhysicsAuthorityGate::ProductionDisabled.authorize(false, true),
        Ok(MovementSource::FreeCamera)
    );
    assert_eq!(
        PhysicsAuthorityGate::CandidateEvidence.authorize(true, true),
        Ok(MovementSource::FreeCamera)
    );
    assert_eq!(
        PhysicsAuthorityGate::CandidateEvidence.authorize(false, false),
        Err(PhysicsAuthorityFault::IncompleteCollisionRegistry)
    );
    assert_eq!(
        PhysicsAuthorityGate::CandidateEvidence.authorize(false, true),
        Ok(MovementSource::Physics)
    );
}

#[test]
fn default_free_camera_never_enqueues_or_sends_after_start_game_and_correction() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    assert_eq!(
        ticker.enqueue_completed_physics(completed_sample(1_001, [100.0, 200.0, 300.0])),
        Err(PhysicsAuthorityFault::Unauthorized)
    );
    ticker.snap_non_authoritative_anchor(1_050, [8.0, 70.0, 9.0]);

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
fn completed_physics_ticks_are_the_only_outbound_enqueue_path() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    for tick in 1_001..=1_020 {
        ticker
            .enqueue_completed_physics(completed_sample(tick, [1.0, 64.0, 2.0]))
            .unwrap();
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
    ticker
        .enqueue_completed_physics(completed_sample(11, [1.0, 2.0, 3.0]))
        .unwrap();
    assert_eq!(ticker.pending_count(), 1);

    // A replacement StartGame explicitly restores the app's current source.
    ticker.set_source(MovementSource::FreeCamera);
    assert_eq!(ticker.pending_count(), 0);
    ticker.reset(2, 1_000, [8.0, 70.0, 9.0]);
    ticker.snap_non_authoritative_anchor(1_050, [10.0, 72.0, 11.0]);

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
    ticker
        .enqueue_completed_physics(completed_sample(1_001, [2.0, 64.0, 3.0]))
        .unwrap();
    let pending = ticker.pop_pending().unwrap();

    ticker.set_source(MovementSource::FreeCamera);
    assert_eq!(ticker.retry_front(pending.clone()), Err(Box::new(pending)));
    assert_eq!(ticker.pending_count(), 0);
}

#[test]
fn tick_snapshots_encode_held_and_edge_flags_and_position_delta() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 41, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    let mut pressed = completed_sample(42, [1.25, 64.0, 1.5]);
    pressed.jumping = true;
    pressed.sprinting = true;

    ticker.enqueue_completed_physics(pressed.clone()).unwrap();
    let first = ticker.pop_pending().unwrap().snapshot;
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

    pressed.tick = 43;
    ticker.enqueue_completed_physics(pressed.clone()).unwrap();
    let held = ticker.pop_pending().unwrap().snapshot;
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

    let released = completed_sample(44, pressed.position);
    ticker.enqueue_completed_physics(released).unwrap();
    let released = ticker.pop_pending().unwrap().snapshot;
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
fn outbox_is_bounded_and_session_reset_discards_stale_ticks_and_input_edges() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    for tick in 11..11 + OUTBOX_CAPACITY as u64 {
        ticker
            .enqueue_completed_physics(completed_sample(tick, [2.0, 3.0, 4.0]))
            .unwrap();
    }
    assert_eq!(ticker.pending_count(), OUTBOX_CAPACITY);
    assert_eq!(ticker.dropped_tick_count(), 0);

    let retry = ticker.pop_pending().unwrap();
    ticker.retry_front(retry).unwrap();
    assert_eq!(ticker.pending_count(), OUTBOX_CAPACITY);

    ticker.reset(2, 5_000, [9.0, 10.0, 11.0]);
    assert_eq!(ticker.session_generation(), 2);
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(ticker.dropped_tick_count(), 0);
    ticker
        .enqueue_completed_physics(completed_sample(5_001, [9.0, 10.0, 11.0]))
        .unwrap();
    let new_session = ticker.pop_pending().unwrap().snapshot;
    assert_eq!(new_session.tick, 5_001);
    assert_eq!(new_session.delta, [0.0; 3]);
    assert_eq!(
        new_session.flags.bits() & PlayerInputFlags::START_JUMPING.bits(),
        0
    );
    ticker.deactivate();
    assert_eq!(
        ticker.enqueue_completed_physics(completed_sample(5_002, [9.0, 10.0, 11.0])),
        Err(PhysicsAuthorityFault::Unauthorized)
    );
    assert_eq!(ticker.pending_count(), 0);
}

#[test]
fn retry_front_rejects_over_capacity_without_losing_the_snapshot() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 0, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    for tick in 1..=OUTBOX_CAPACITY as u64 {
        ticker
            .enqueue_completed_physics(completed_sample(tick, [0.0; 3]))
            .unwrap();
    }
    let pending = ticker.pop_pending().unwrap();
    ticker.retry_front(pending).unwrap();
    let duplicate = ticker.peek_pending().unwrap().clone();
    let error = ticker.retry_front(duplicate.clone()).unwrap_err();
    assert_eq!(*error, duplicate);
    assert_eq!(ticker.pending_count(), OUTBOX_CAPACITY);
}

#[test]
fn bounded_flush_restores_the_exact_front_snapshot_when_transport_is_full() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(completed_sample(11, [1.0, 2.0, 3.0]))
        .unwrap();
    ticker
        .enqueue_completed_physics(completed_sample(12, [1.5, 2.0, 3.0]))
        .unwrap();
    let expected = ticker.peek_pending().unwrap().clone();

    let error = flush_player_auth_inputs(&mut ticker, 8, |_packet| Err("full")).unwrap_err();
    assert!(matches!(error, MovementSendError::Transport("full")));
    assert_eq!(ticker.pending_count(), 2);
    assert_eq!(ticker.peek_pending().unwrap(), &expected);
    assert_eq!(ticker.sent_physics_packet_count(), 0);
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::TransportRestored
    );
    ticker.note_full_restore();
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::FullRestored
    );

    let mut sent_packets = 0;
    let sent = flush_player_auth_inputs(&mut ticker, 8, |_packet| {
        sent_packets += 1;
        Ok::<_, &str>(())
    })
    .unwrap();
    assert_eq!(sent, 2);
    assert_eq!(sent_packets, 2);
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(ticker.sent_physics_packet_count(), 2);
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::Drained
    );
}

#[test]
fn keyboard_diagonal_is_normalized_without_losing_the_raw_vector() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 0, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    let mut diagonal = completed_sample(1, [0.0; 3]);
    diagonal.move_vector = [1.0, 1.0];
    ticker.enqueue_completed_physics(diagonal).unwrap();
    let snapshot = ticker.pop_pending().unwrap().snapshot;

    let component = 1.0_f32 / 2.0_f32.sqrt();
    assert!((snapshot.move_vector[0] - component).abs() < 1e-6);
    assert!((snapshot.move_vector[1] - component).abs() < 1e-6);
    assert_eq!(snapshot.raw_move_vector, [1.0, 1.0]);
    assert_eq!(snapshot.analogue_move_vector, snapshot.move_vector);
}

struct Floor;

impl CollisionWorld for Floor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(64.0, 1.0, 64.0));
        Ok(CollisionQuery::synthetic(
            floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }
}

struct VersionedFloor(u8);

impl CollisionWorld for VersionedFloor {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(64.0, 1.0, 64.0));
        Ok(CollisionQuery {
            value: floor
                .intersects(query)
                .then_some(floor)
                .into_iter()
                .collect(),
            identity: fixture_world_identity(self.0),
        })
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        let mut sample = Floor.block_physics(block)?;
        sample.identity = fixture_world_identity(self.0);
        Ok(sample)
    }
}

struct UnavailableWorld;

impl CollisionWorld for UnavailableWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
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
fn completed_physics_ticks_enqueue_exact_positions_ticks_modes_and_edges() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 40, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(100),
        forward_physics_input(),
        PhysicsSampleContext {
            pitch: 12.0,
            head_yaw: 180.0,
            camera_orientation: [0.0, 0.0, 1.0],
            input_mode: PlayerInputMode::GamePad,
        },
        &Floor,
    );
    assert_eq!(frame.samples.len(), 2);
    assert_eq!(frame.samples[0].tick, 41);
    assert_eq!(frame.samples[1].tick, 42);
    assert_eq!(frame.samples[0].input_mode, PlayerInputMode::GamePad);
    assert_eq!(frame.samples[0].position[1], 2.620_01);

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 40, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let first = ticker.pop_pending().unwrap().snapshot;
    let second = ticker.pop_pending().unwrap().snapshot;
    assert_eq!((first.tick, second.tick), (41, 42));
    assert_eq!(first.input_mode, PlayerInputMode::GamePad);
    assert_eq!(second.delta[1], second.position[1] - first.position[1]);
}

#[test]
fn multi_tick_catch_up_exposes_every_completed_tick_to_evidence_before_send() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext {
            input_mode: PlayerInputMode::GamePad,
            ..PhysicsSampleContext::default()
        },
        &Floor,
    );
    assert_eq!(frame.samples.len(), 3);

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    let evidence = ticker.take_tick_evidence();
    assert_eq!(
        evidence
            .iter()
            .map(|sample| sample.tick)
            .collect::<Vec<_>>(),
        [101, 102, 103]
    );
    assert!(
        evidence.iter().all(|sample| sample.session_generation == 7
            && sample.input_mode == PlayerInputMode::GamePad)
    );
    assert!(ticker.take_tick_evidence().is_empty());
}

#[test]
fn catch_up_evidence_cursor_does_not_repeat_restored_full_retry_ticks() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);

    let catch_up = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &Floor,
    );
    for sample in catch_up.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    assert_eq!(
        ticker
            .take_tick_evidence()
            .iter()
            .map(|sample| sample.tick)
            .collect::<Vec<_>>(),
        [101, 102, 103]
    );
    let full = flush_player_auth_inputs(&mut ticker, 8, |_packet| Err("full")).unwrap_err();
    assert!(matches!(full, MovementSendError::Transport("full")));

    let next = physics.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &Floor,
    );
    assert_eq!(next.samples.len(), 1);
    ticker
        .enqueue_completed_physics(next.samples.into_iter().next().unwrap())
        .unwrap();

    assert_eq!(
        ticker
            .take_tick_evidence()
            .iter()
            .map(|sample| sample.tick)
            .collect::<Vec<_>>(),
        [104]
    );
    assert!(ticker.take_tick_evidence().is_empty());
}

#[test]
fn retained_correction_replays_physics_and_replaces_only_unsent_fifo_ticks() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    assert_eq!(frame.samples.len(), 3);

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let sent = ticker.pop_pending().unwrap();
    assert_eq!(sent.snapshot.tick, 101);
    let before = ticker.pending_samples();

    let outcome = reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [0.25, 2.620_01, 0.0],
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
    assert_eq!(physics.state().unwrap().tick, 103);
    assert_eq!(ticker.next_tick(), 104);
    let after = ticker.pending_samples();
    assert_eq!(
        after
            .iter()
            .map(|pending| pending.snapshot.tick)
            .collect::<Vec<_>>(),
        [102, 103]
    );
    assert!(after.iter().all(|pending| pending.session_generation == 7));
    assert_eq!(
        after
            .iter()
            .map(|pending| &pending.world_identity)
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|pending| &pending.world_identity)
            .collect::<Vec<_>>()
    );
    assert_ne!(after[0].snapshot.position, before[0].snapshot.position);
    assert_ne!(after[1].snapshot.position, before[1].snapshot.position);
}

#[test]
fn replay_world_identity_change_records_fault_before_deauthorizing_atomically() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(
        Duration::from_millis(100),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(9, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [0.25, 2.620_01, 0.0],
            101,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(2),
        ),
        Err(PhysicsAuthorityFault::ReplayWorldIdentityMismatch { tick: 102 })
    );
    assert!(!physics.is_active());
    assert!(!ticker.physics_is_authorized());
    assert_eq!(ticker.pending_count(), 0);
    let fault = ticker.take_authority_fault().unwrap();
    assert_eq!(fault.session_generation, 9);
    assert_eq!(
        fault.fault,
        PhysicsAuthorityFault::ReplayWorldIdentityMismatch { tick: 102 }
    );
    assert_eq!(fault.pending_count, 2);
}

#[test]
fn replay_request_without_a_retained_tick_fails_closed_instead_of_snapping() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(
        Duration::from_millis(100),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(13, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [4.0, 70.620_01, 5.0],
            103,
            false,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(1),
        ),
        Err(PhysicsAuthorityFault::CorrectionNotRetained { tick: 103 })
    );
    assert!(!physics.is_active());
    assert!(!ticker.physics_is_authorized());
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(
        ticker.take_authority_fault().unwrap().fault,
        PhysicsAuthorityFault::CorrectionNotRetained { tick: 103 }
    );
}

#[test]
fn replay_rejects_a_queued_sample_whose_collision_identity_changed() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(
        Duration::from_millis(100),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(17, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    ticker
        .outbox
        .back_mut()
        .expect("second completed sample is queued")
        .world_identity = fixture_world_identity(2);

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [0.25, 2.620_01, 0.0],
            101,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(1),
        ),
        Err(PhysicsAuthorityFault::PendingWorldIdentityMismatch { tick: 102 })
    );
    assert!(!physics.is_active());
    assert!(!ticker.physics_is_authorized());
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(
        ticker.take_authority_fault().unwrap().fault,
        PhysicsAuthorityFault::PendingWorldIdentityMismatch { tick: 102 }
    );
}

#[test]
fn replay_tick_alignment_mismatch_fails_closed_with_expected_and_actual_ticks() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(
        Duration::from_millis(100),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(23, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    ticker.next_tick = 999;

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [0.25, 2.620_01, 0.0],
            101,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(1),
        ),
        Err(PhysicsAuthorityFault::PendingTickMismatch {
            expected: 103,
            actual: 999,
        })
    );
    assert!(!physics.is_active());
    assert!(!ticker.physics_is_authorized());
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(
        ticker.take_authority_fault().unwrap().fault,
        PhysicsAuthorityFault::PendingTickMismatch {
            expected: 103,
            actual: 999,
        }
    );
}

#[test]
fn stale_explicit_snap_preserves_monotonic_ticker_and_physics_alignment() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance(
        Duration::from_millis(50),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    let mut ticker = MovementTicker::default();
    ticker.reset(3, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(frame.samples[0].clone())
        .unwrap();

    let outcome = reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [8.0, 71.620_01, 9.0],
        0,
        false,
        PhysicsCorrectionMode::Snap,
        &VersionedFloor(2),
    )
    .unwrap();

    assert_eq!(outcome, PhysicsCorrectionOutcome::Snapped { tick: 101 });
    assert_eq!(physics.state().unwrap().tick, 101);
    assert_eq!(ticker.next_tick(), 102);
    assert_eq!(ticker.pending_count(), 0);
    assert!(ticker.physics_is_authorized());
}

#[test]
fn completed_sample_overflow_never_drops_oldest_and_records_one_bounded_fault() {
    let mut ticker = MovementTicker::default();
    ticker.reset(11, 0, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    for tick in 1..=OUTBOX_CAPACITY as u64 {
        ticker
            .enqueue_completed_physics(completed_sample(tick, [tick as f32, 0.0, 0.0]))
            .unwrap();
    }
    let oldest = ticker.peek_pending().unwrap().snapshot;

    assert_eq!(
        ticker.enqueue_completed_physics(completed_sample(
            OUTBOX_CAPACITY as u64 + 1,
            [99.0, 0.0, 0.0],
        )),
        Err(PhysicsAuthorityFault::OutboxOverflow)
    );
    assert_eq!(oldest.tick, 1);
    assert_eq!(ticker.dropped_tick_count(), 0);
    assert!(!ticker.physics_is_authorized());
    assert_eq!(ticker.pending_count(), 0);
    let fault = ticker.take_authority_fault().unwrap();
    assert_eq!(fault.session_generation, 11);
    assert_eq!(fault.fault, PhysicsAuthorityFault::OutboxOverflow);
    assert_eq!(fault.pending_count, OUTBOX_CAPACITY);
    assert!(ticker.take_authority_fault().is_none());
}

#[test]
fn explicit_deactivation_does_not_erase_a_pending_authority_fault() {
    let mut ticker = MovementTicker::default();
    ticker.reset(19, 0, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    assert_eq!(
        ticker.enqueue_completed_physics(completed_sample(2, [1.0, 0.0, 0.0])),
        Err(PhysicsAuthorityFault::TickMismatch {
            expected: 1,
            actual: 2,
        })
    );

    ticker.deactivate();

    let fault = ticker.take_authority_fault().unwrap();
    assert_eq!(fault.session_generation, 19);
    assert_eq!(
        fault.fault,
        PhysicsAuthorityFault::TickMismatch {
            expected: 1,
            actual: 2,
        }
    );
}

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
    let breg = include_bytes!("../../../crates/assets/data/block-registry-v1001.bin");
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
