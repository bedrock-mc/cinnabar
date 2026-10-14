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
    flush_player_auth_inputs(&mut ticker, 2, None, |_, packet| {
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

