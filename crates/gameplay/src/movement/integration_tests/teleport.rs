/// Supplies still water at an airborne teleport destination above the test floor.
struct TeleportWater;

impl CollisionWorld for TeleportWater {
    /// Uses the same immutable registry as every floor and water sample.
    fn registry_identity(&self) -> CollisionRegistryIdentity {
        VersionedFloor(1).registry_identity()
    }

    /// Keeps collision identity and the floor shared with the movement fixtures.
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        VersionedFloor(1).collision_boxes(query)
    }

    /// Fills every cell above the floor with source water and no current.
    fn block_physics(&self, block: [i32; 3]) -> Result<sim::BlockPhysicsSample, WorldQueryError> {
        let mut sample = VersionedFloor(1).block_physics(block)?;
        if block[1] >= 1 {
            sample.layers[0].flags = sim::BlockPhysicsFlags::from_bits(
                sample.layers[0].flags.bits() | sim::BlockPhysicsFlags::WATER.bits(),
            )
            .unwrap();
            sample.layers[0].fluid_height_blocks = 1.0;
        }
        Ok(sample)
    }
}

/// Applies a tick-zero teleport and consumes only the pre-anchor frame delta.
fn snapped_teleport(world: &impl CollisionWorld) -> (LocalPhysicsController, MovementTicker) {
    let (mut physics, mut ticker) = walked_physics(4);
    let target = [0.5, 10.0 + protocol::PLAYER_NETWORK_OFFSET, 0.5];
    super::reconcile_move_player_teleport(&mut ticker, &mut physics, target, 0, false, world)
        .unwrap();
    assert!(
        physics
            .advance(Duration::from_millis(50), MovementInput::default(), world)
            .samples
            .is_empty()
    );
    (physics, ticker)
}

/// The teleport acknowledgement holds the destination despite walking and falling.
#[test]
fn snapped_teleport_skips_only_the_first_air_travel_tick() {
    let world = VersionedFloor(1);
    let (mut physics, mut ticker) = snapped_teleport(&world);
    let target = physics.network_position().unwrap();
    let input = forward_physics_input();
    let frame = physics.advance(Duration::from_millis(100), input, &world);
    assert_eq!(frame.samples.len(), 2);
    assert_eq!(frame.samples[0].position, target);
    assert_eq!(frame.samples[0].movement, [0.0; 3]);
    assert_eq!(frame.samples[0].velocity, [0.0; 3]);
    assert_eq!(frame.samples[0].move_vector, [0.0, 1.0]);
    assert!(!frame.samples[0].grounded_after_tick);

    let mut ordinary = LocalPhysicsController::default();
    ordinary.reanchor_network_position(target, frame.samples[0].tick, false);
    let expected = ordinary.advance(Duration::from_millis(50), input, &world);
    assert_eq!(frame.samples[1].position, expected.samples[0].position);
    assert_eq!(frame.samples[1].velocity, expected.samples[0].velocity);

    ticker.note_server_teleport(super::teleport_ack::ServerTeleportKind::MovePlayer);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let mut packets = Vec::new();
    flush_player_auth_inputs(&mut ticker, 2, Some(evidence_context()), |_, packet| {
        packets.push(packet.clone());
        Ok::<_, &str>(())
    })
    .unwrap();
    let ack = protocol::player_auth_input_trace_sample(&packets[0]).unwrap();
    assert!(ack.flag_names.contains(&"HandledTeleport"));
    assert_eq!(ack.position, target);
    assert_eq!(ack.pos_delta, [0.0; 3]);
}

/// Water contact survives the stationary tick and selects the next liquid travel.
#[test]
fn snapped_teleport_samples_water_before_travel_resumes() {
    let (mut physics, _) = snapped_teleport(&TeleportWater);
    let target = physics.network_position().unwrap();
    let frame = physics.advance(
        Duration::from_millis(50),
        MovementInput::default(),
        &TeleportWater,
    );
    assert_eq!(frame.samples[0].position, target);
    assert_eq!(frame.samples[0].velocity, [0.0; 3]);
    assert!(physics.in_water());
    let next = physics.advance(
        Duration::from_millis(50),
        MovementInput::default(),
        &TeleportWater,
    );
    assert_eq!(next.samples[0].position, target);
    assert_eq!(next.samples[0].movement, [0.0; 3]);
    assert_eq!(next.samples[0].velocity, [0.0, -0.005, 0.0]);
}

/// Missing terrain defers the single skipped tick until prediction succeeds.
#[test]
fn snapped_teleport_keeps_travel_suppression_across_a_blocked_tick() {
    let (mut physics, _) = snapped_teleport(&VersionedFloor(1));
    let target = physics.network_position().unwrap();
    let blocked = physics.advance(
        Duration::from_millis(50),
        forward_physics_input(),
        &UnavailableWorld,
    );
    assert!(blocked.blocked.is_some());
    assert!(blocked.samples.is_empty());
    let frame = physics.advance(
        Duration::from_millis(100),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    assert_eq!(frame.samples.len(), 2);
    assert_eq!(frame.samples[0].position, target);
    assert_eq!(frame.samples[0].velocity, [0.0; 3]);
    assert_ne!(frame.samples[1].position, target);
}

/// Floor recovery precedes resumed travel even within a multi-tick render frame.
#[test]
fn snapped_teleport_recovers_floor_overlap_before_catch_up_travel() {
    let world = VersionedFloor(1);
    let (mut physics, mut ticker) = walked_physics(4);
    let target = [0.5, 0.9 + protocol::PLAYER_NETWORK_OFFSET, 0.5];
    super::reconcile_move_player_teleport(&mut ticker, &mut physics, target, 0, true, &world)
        .unwrap();
    physics.advance(Duration::from_millis(50), MovementInput::default(), &world);

    let frame = physics.advance(Duration::from_millis(150), MovementInput::default(), &world);
    assert_eq!(frame.samples.len(), 3);
    assert_eq!(frame.samples[0].position, target);
    for sample in &frame.samples {
        assert_eq!(sample.movement, [0.0; 3]);
        assert!(sample.grounded_after_tick);
    }
    assert!((physics.state().unwrap().position.y - 1.0).abs() < 1e-5);

    let expected = physics.state().cloned();
    let skipped_tick = frame.samples[0].tick;
    for sample in &frame.samples {
        ticker.enqueue_completed_physics(sample.clone()).unwrap();
    }
    let outcome = super::reconcile_physics_anchor(
        &mut ticker,
        &mut physics,
        super::PhysicsAnchor {
            network_position: target,
            tick: skipped_tick,
            on_ground: true,
            velocity: Some([0.0; 3]),
        },
        PhysicsCorrectionMode::ReplayIfRetained,
        &world,
    )
    .unwrap();
    assert!(matches!(
        outcome,
        PhysicsCorrectionOutcome::Replayed {
            replayed_ticks: 2,
            ..
        }
    ));
    assert_eq!(physics.state(), expected.as_ref());
    for sample in &frame.samples[1..] {
        let replayed = physics.sample_at(sample.tick).unwrap();
        assert_eq!(replayed.movement, [0.0; 3]);
        assert!(replayed.grounded_after_tick);
    }
}

/// Session reanchoring discards a teleport marker belonging to the old timeline.
#[test]
fn reanchoring_clears_pending_teleport_travel_suppression() {
    let (mut physics, _) = snapped_teleport(&VersionedFloor(1));
    let target = physics.network_position().unwrap();
    physics.reanchor_network_position(target, 200, false);
    let frame = physics.advance(
        Duration::from_millis(50),
        forward_physics_input(),
        &VersionedFloor(1),
    );
    assert_ne!(frame.samples[0].position, target);
    assert_ne!(frame.samples[0].velocity, [0.0; 3]);
}

/// Replaying a captured stationary tick in clear air must not turn it back into travel.
#[test]
fn correction_replay_preserves_the_captured_travel_suppression() {
    let world = VersionedFloor(1);
    let (mut physics, mut ticker) = walked_physics(2);
    physics.skip_next_travel_tick();
    for sample in physics
        .advance(Duration::from_millis(100), forward_physics_input(), &world)
        .samples
    {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let target = [1.0, 10.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0];
    super::reconcile_physics_anchor(
        &mut ticker,
        &mut physics,
        super::PhysicsAnchor {
            network_position: target,
            tick: 102,
            on_ground: false,
            velocity: Some([0.0; 3]),
        },
        PhysicsCorrectionMode::ReplayIfRetained,
        &world,
    )
    .unwrap();
    assert_eq!(physics.sample_at(103).unwrap().position, target);
    assert_eq!(physics.sample_at(103).unwrap().velocity, [0.0; 3]);
    let mut expected = LocalPhysicsController::default();
    expected.reanchor_network_position(target, 102, false);
    expected.skip_next_travel_tick();
    expected.advance(Duration::from_millis(100), forward_physics_input(), &world);
    assert_eq!(physics.state(), expected.state());
    assert_eq!(physics.sample_at(104), expected.sample_at(104));
}
