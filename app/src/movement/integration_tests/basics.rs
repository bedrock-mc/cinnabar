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
fn free_camera_authority_deactivates_prepared_and_reanchored_local_physics() {
    let mut movement = MovementTicker::default();
    let mut local_physics = LocalPhysicsController::default();
    local_physics.reanchor_network_position([8.0, 72.62, -4.0], 100, false);
    assert!(local_physics.is_active());

    assert_eq!(
        PhysicsAuthorityGate::ProductionDisabled.apply_start_game(
            false,
            true,
            &mut movement,
            &mut local_physics,
        ),
        Ok(MovementSource::FreeCamera)
    );
    assert_eq!(movement.source(), MovementSource::FreeCamera);
    assert!(!local_physics.is_active());

    local_physics.reanchor_network_position([9.0, 73.62, -3.0], 101, true);
    movement.snap_non_authoritative_anchor(101, [9.0, 73.62, -3.0]);
    movement.enforce_local_physics_authority(&mut local_physics);
    assert!(!local_physics.is_active());
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
    let flushed = flush_player_auth_inputs(&mut ticker, 8, None, |_identity, _packet| {
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
    let flushed = flush_player_auth_inputs(
        &mut ticker,
        usize::MAX,
        Some(evidence_context()),
        |_identity, _packet| {
            sent_packets += 1;
            Ok::<_, &str>(())
        },
    )
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
    let flushed = flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |_identity, _packet| {
            sent_packets += 1;
            Ok::<_, &str>(())
        },
    )
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

/// A versioned floor plus a wall the forward input runs into, so retained
/// prediction state actually carries a horizontal axis collision.
pub(super) struct VersionedWall(pub(super) u8);

impl CollisionWorld for VersionedWall {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(64.0, 1.0, 64.0));
        let wall = Aabb::new(Vec3::new(-64.0, 1.0, -2.0), Vec3::new(64.0, 4.0, -1.5));
        Ok(CollisionQuery {
            value: [floor, wall]
                .into_iter()
                .filter(|shape| shape.intersects(query))
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

pub(super) fn forward_physics_input() -> MovementInput {
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
