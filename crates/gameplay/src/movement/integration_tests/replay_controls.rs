/// UI input suppression leaves the independently controlled view intact.
#[test]
fn inactive_movement_preserves_view_yaw() {
    let input = physics_movement_input([1.0, 1.0], 90.0, false, true, true, true, None);
    assert_eq!(input.yaw_degrees, 90.0);
    assert_eq!([input.strafe, input.forward], [0.0; 2]);
    assert!(!input.jumping && !input.sneaking && !input.sprinting);
}

/// Replayed controls must reach unsent packets and the next tick's edge detector.
#[test]
fn replayed_sneak_rebuilds_outbound_controls_and_following_edges() {
    let (mut physics, mut ticker) = walked_physics(4);
    let update = flags(|flags| flags.sneaking = Some(true));
    assert_eq!(physics.apply_server_movement_flags(102, update), Some(102));
    reconcile_timeline_rewind(&mut ticker, &mut physics, 102, &VersionedFloor(1)).unwrap();
    let pending = ticker.pending_snapshots();
    assert_eq!(pending.len(), 2);
    for (index, snapshot) in pending.iter().enumerate() {
        assert_eq!(snapshot.move_vector, [0.0, 0.3]);
        assert_ne!(snapshot.flags.bits() & PlayerInputFlags::SNEAKING.bits(), 0);
        assert_eq!(snapshot.flags.bits() & PlayerInputFlags::START_SNEAKING.bits() != 0, index == 0);
        assert_eq!(snapshot.flags.bits() & PlayerInputFlags::SNEAK_CURRENT_RAW.bits(), 0);
    }
    let next = run_one_tick(&mut physics, &VersionedFloor(1));
    ticker.enqueue_completed_physics(next).unwrap();
    let next = ticker.pending_snapshots().pop().unwrap();
    assert_ne!(next.flags.bits() & PlayerInputFlags::STOP_SNEAKING.bits(), 0);
}

#[test]
fn nonbinary_primary_bits_and_captured_directions_survive_replay_replacement() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut input = physics_movement_input([0.7, 0.9], 0.0, true, false, true, false, None);
    input.item_use_movement_modifier = Some(f64::from(0.7_f32));
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        input,
        PhysicsSampleContext {
            raw_move_vector: [-1.0, -1.0],
            analogue_move_vector: [-1.0, -1.0],
            ..Default::default()
        },
        &VersionedFloor(1),
    );
    assert_eq!(frame.samples.len(), 3);
    let factor = 0.7_f32 * 0.3_f32;
    let expected = [(0.7_f32 * factor).to_bits(), (0.9_f32 * factor).to_bits()];
    let wire_expected = [(-0.7_f32 * factor).to_bits(), (0.9_f32 * factor).to_bits()];
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    ticker.pop_pending().unwrap();
    let before = ticker.pending_samples();
    let confirmation = ticker.sent_confirmation(101);
    let plan = physics
        .clone()
        .apply_correction(
            super::PhysicsAnchor {
                network_position: [0.25, 2.620_01, 0.0],
                tick: 101,
                on_ground: true,
                velocity: None,
            },
            PhysicsCorrectionMode::ReplayIfRetained,
            confirmation.as_ref(),
            &VersionedFloor(1),
        )
        .unwrap();
    assert_eq!(
        plan.outcome,
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: 101,
            replayed_ticks: 2,
        }
    );
    assert_eq!(plan.replayed_samples.len(), before.len());
    let mask = PlayerInputFlags::UP | PlayerInputFlags::RIGHT | PlayerInputFlags::UP_LEFT;
    for (live, retained) in before.iter().zip(&plan.replayed_samples) {
        assert_eq!(retained.tick, live.snapshot.tick);
        assert_eq!(retained.move_vector.map(f32::to_bits), expected);
        assert_eq!(
            retained.processed.direction_flags.unwrap().bits() & mask.bits(),
            live.snapshot.flags.bits() & mask.bits()
        );
        assert_eq!(retained.raw_move_vector, [-1.0, -1.0]);
        assert_eq!(retained.analogue_move_vector, [-1.0, -1.0]);
    }
    // Captured input stays immutable; only replay-owned output is corrupted.
    for (pending, retained) in ticker.outbox.iter_mut().zip(&plan.replayed_samples) {
        pending.snapshot.position = [99.0; 3];
        pending.snapshot.delta = [99.0; 3];
        pending.snapshot.flags = pending
            .snapshot
            .flags
            .with_mask(
                PlayerInputFlags::HORIZONTAL_COLLISION,
                !retained.horizontal_collision,
            )
            .with_mask(
                PlayerInputFlags::VERTICAL_COLLISION,
                !retained.vertical_collision,
            )
            .with_mask(
                PlayerInputFlags::JUMPING,
                !retained.jumping,
            );
    }
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [0.25, 2.620_01, 0.0],
        101,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();
    let after = ticker.pending_samples();
    assert_eq!(before.len(), 2);
    assert_eq!(after.len(), before.len());
    for ((live, replayed), retained) in before.into_iter().zip(after).zip(plan.replayed_samples) {
        assert_eq!(replayed.snapshot.position, retained.position);
        assert_eq!(replayed.snapshot.delta, retained.velocity);
        assert_ne!(replayed.snapshot.position, [99.0; 3]);
        assert_ne!(replayed.snapshot.delta, [99.0; 3]);
        assert_ne!(replayed.snapshot.position, live.snapshot.position);
        assert_eq!(replayed.evidence.network_position, retained.position);
        for (flag, expected_value) in [
            (
                PlayerInputFlags::HORIZONTAL_COLLISION,
                retained.horizontal_collision,
            ),
            (
                PlayerInputFlags::VERTICAL_COLLISION,
                retained.vertical_collision,
            ),
            (
                PlayerInputFlags::JUMPING,
                retained.jumping,
            ),
        ] {
            assert_eq!(
                replayed.snapshot.flags.bits() & flag.bits() != 0,
                expected_value
            );
        }
        assert_eq!(live.snapshot.move_vector.map(f32::to_bits), wire_expected);
        assert_eq!(replayed.snapshot.move_vector.map(f32::to_bits), wire_expected);
        assert_eq!(
            replayed.snapshot.flags.bits() & mask.bits(),
            live.snapshot.flags.bits() & mask.bits()
        );
        assert_eq!(
            replayed.snapshot.flags.bits() & PlayerInputFlags::UP_LEFT.bits(),
            0
        );
        assert_eq!(replayed.snapshot.raw_move_vector, [1.0, -1.0]);
        assert_eq!(replayed.snapshot.analogue_move_vector, [1.0, -1.0]);
    }
}

/// A changed correction anchor must re-evaluate forced sneak, including repeated replays.
#[test]
fn correction_recomputes_pose_from_the_corrected_position() {
    struct LowRoof;
    impl CollisionWorld for LowRoof {
        fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            let mut boxes = Floor.collision_boxes(query)?;
            let roof = Aabb::new(Vec3::new(2.0, 2.5, -2.0), Vec3::new(4.0, 3.0, 2.0));
            if roof.intersects(query) { boxes.value.push(roof); }
            Ok(boxes)
        }
    }
    let mut physics = LocalPhysicsController::default();
    let y = 1.0 + protocol::PLAYER_NETWORK_OFFSET;
    physics.reanchor_network_position([0.0, y, 0.0], 100, true);
    let frame = physics.advance_with_context(Duration::from_millis(150), MovementInput::default(), PhysicsSampleContext::default(), &LowRoof);
    assert_eq!(frame.samples.len(), 3);
    assert!(frame.samples.iter().all(|sample| !sample.sneaking));
    let anchor = super::PhysicsAnchor { network_position: [3.0, y, 0.0], tick: 101, on_ground: true, velocity: None };
    let first = physics.apply_correction(anchor, PhysicsCorrectionMode::ReplayIfRetained, None, &LowRoof).unwrap();
    assert_eq!(first.replayed_samples.len(), 2);
    assert!(first.replayed_samples.iter().all(|sample| sample.sneaking && sample.processed.forced_sneak));
    let second = physics.apply_correction(anchor, PhysicsCorrectionMode::ReplayIfRetained, None, &LowRoof).unwrap();
    assert_eq!(first.replayed_samples, second.replayed_samples);
}

/// Correction replay uses historical ceiling changes even after the live palette is gone.
#[test]
fn correction_replays_controller_modes_against_historical_palettes() {
    let mut registry = sim::CollisionRegistry::new();
    registry.register(0, []).unwrap();
    registry.register(1, [Aabb::new(Vec3::ZERO, Vec3::ONE)]).unwrap();
    registry.register(2, [Aabb::new(Vec3::new(0.0, 0.5, 0.0), Vec3::ONE)]).unwrap();
    let mut store = world::ChunkStore::new();
    let key = world::SubChunkKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key).unwrap();
    store.update_block(key, world::BlockUpdate::new(8, 7, 8, 0, 1), 0).unwrap();
    let mut physics = LocalPhysicsController::default();
    let position = [8.5, 8.0 + protocol::PLAYER_NETWORK_OFFSET, 8.5];
    physics.reanchor_network_position(position, 100, true);
    let mut original = Vec::new();
    for tick in 1..=3 {
        if tick > 1 { store.update_block(key, world::BlockUpdate::new(8, 9, 8, 0, if tick == 2 { 2 } else { 0 }), 0).unwrap(); }
        let frame = physics.advance_with_context(Duration::from_millis(50), MovementInput::default(), PhysicsSampleContext::default(), &sim::PaletteWorld::new(&store, &registry, 0));
        assert_eq!(frame.samples.len(), 1, "{:?}", frame.blocked);
        original.extend(frame.samples);
    }
    assert!(!original[0].sneaking && original[1].sneaking && !original[2].sneaking);
    store.evict_chunk(world::ChunkKey::new(0, 0, 0));
    let plan = physics.apply_correction(super::PhysicsAnchor { network_position: original[0].position, tick: 101, on_ground: true, velocity: Some(original[0].velocity) }, PhysicsCorrectionMode::ReplayIfRetained, None, &sim::PaletteWorld::new(&store, &registry, 0)).unwrap();
    assert_eq!(plan.replayed_samples, original[1..]);
}

/// Mounting ends the player's jump arc in both live prediction and repeated replay.
#[test]
fn replayed_mount_closes_the_airborne_jump_arc() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 1.0 + protocol::PLAYER_NETWORK_OFFSET, 0.0], 100, true);
    let jump = physics.advance_with_context(Duration::from_millis(50), MovementInput { jumping: true, ..Default::default() }, PhysicsSampleContext::default(), &VersionedFloor(1)).samples.remove(0);
    assert!(jump.processed.jump_arc_active);
    let ride = PhysicsSampleContext {
        mode_intent: super::ModeIntent { ride: Some(super::RideKind::Boat), ride_seat: Some([0.0, 3.0, 0.0]), ..Default::default() },
        ..Default::default()
    };
    let mounted = physics.advance_with_context(Duration::from_millis(50), MovementInput::default(), ride, &VersionedFloor(1)).samples.remove(0);
    assert!(!mounted.processed.jump_arc_active);
    let anchor = super::PhysicsAnchor { network_position: jump.position, tick: jump.tick, on_ground: false, velocity: Some(jump.velocity) };
    for _ in 0..2 {
        let replay = physics.apply_correction(anchor, PhysicsCorrectionMode::ReplayIfRetained, None, &VersionedFloor(1)).unwrap();
        assert_eq!(replay.replayed_samples, std::slice::from_ref(&mounted));
    }
}
