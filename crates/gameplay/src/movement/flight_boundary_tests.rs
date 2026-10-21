use super::*;

/// Loads an empty column as authoritative collision terrain.
fn load_air_column(store: &mut world::ChunkStore, x: i32) {
    store
        .mark_chunk_loaded(world::ChunkKey::new(0, x, 0))
        .unwrap();
}

#[test]
fn movement_waits_for_terrain_at_an_absent_teleport_destination() {
    let mut registry = sim::CollisionRegistry::new();
    registry.register(0, []).unwrap();
    for intent in [ModeIntent::default(), boundary_flight()] {
        let mut store = world::ChunkStore::new();
        load_air_column(&mut store, 0);
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position(
            [24.0, 80.0 + protocol::PLAYER_NETWORK_OFFSET, 8.0],
            100,
            false,
        );
        let anchored = physics.state().unwrap().clone();
        for _ in 0..20 {
            let frame = physics.advance_with_context(
                TICK,
                MovementInput::default(),
                PhysicsSampleContext {
                    mode_intent: intent,
                    ..Default::default()
                },
                &sim::PaletteWorld::for_movement(&store, &registry, 0),
            );
            assert_eq!(frame.completed_ticks, 0);
            assert!(matches!(
                frame.blocked,
                Some(sim::SimulationError::World(WorldQueryError::UnloadedChunk(
                    _
                )))
            ));
            assert_eq!(physics.state(), Some(&anchored));
        }
        load_air_column(&mut store, 1);
        let frame = physics.advance_with_context(
            TICK,
            MovementInput::default(),
            PhysicsSampleContext {
                mode_intent: intent,
                ..Default::default()
            },
            &sim::PaletteWorld::for_movement(&store, &registry, 0),
        );
        assert_eq!(frame.completed_ticks, 1);
        assert!(frame.blocked.is_none());
    }
}

/// Advances real ability flight into a missing neighbouring column.
fn reach_missing_column(
    registry: &sim::CollisionRegistry,
    store: &world::ChunkStore,
) -> LocalPhysicsController {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(
        [8.0, 80.0 + protocol::PLAYER_NETWORK_OFFSET, 8.0],
        100,
        false,
    );
    let world = sim::PaletteWorld::new(store, registry, 0, Default::default());
    for _ in 0..100 {
        let frame = physics.advance_with_context(
            TICK,
            MovementInput {
                forward: 1.0,
                yaw_degrees: -90.0,
                ..Default::default()
            },
            PhysicsSampleContext {
                mode_intent: boundary_flight(),
                ..Default::default()
            },
            &world,
        );
        if matches!(
            frame.blocked,
            Some(sim::SimulationError::World(WorldQueryError::UnloadedChunk(
                _
            )))
        ) {
            assert_eq!(physics.mode(), MovementMode::Flying);
            return physics;
        }
        assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    }
    panic!("flight did not reach the missing column");
}

/// Supplies creative flight abilities through the production selector's authority input.
fn boundary_flight() -> ModeIntent {
    ModeIntent {
        can_fly: true,
        flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(true),
        creative_flight: true,
        ..Default::default()
    }
}

#[test]
fn same_dimension_teleport_waits_for_the_occupied_column_before_predicting() {
    for x in [24.0_f32, 16.125, -0.125] {
        let mut registry = sim::CollisionRegistry::new();
        registry.register(0, []).unwrap();
        let mut store = world::ChunkStore::new();
        load_air_column(&mut store, 0);
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position(
            [8.0, 80.0 + protocol::PLAYER_NETWORK_OFFSET, 8.0],
            100,
            false,
        );
        step(
            &mut physics,
            MovementInput::default(),
            ModeIntent::default(),
            &sim::PaletteWorld::for_movement(&store, &registry, 0),
        );
        physics.reanchor_network_position(
            [x, 80.0 + protocol::PLAYER_NETWORK_OFFSET, 8.0],
            200,
            false,
        );
        let anchor = physics.state().unwrap().clone();
        let chunk = world::ChunkKey::new(0, x.floor() as i32 >> 4, 0);
        for _ in 0..10 {
            let frame = physics.advance_with_context(
                TICK,
                MovementInput {
                    forward: 1.0,
                    ..Default::default()
                },
                PhysicsSampleContext::default(),
                &sim::PaletteWorld::for_movement(&store, &registry, 0),
            );
            assert_eq!(frame.completed_ticks, 0);
            assert!(matches!(
                frame.blocked,
                Some(sim::SimulationError::World(WorldQueryError::UnloadedChunk(key)))
                    if key == chunk
            ));
            assert_eq!(physics.state(), Some(&anchor));
            assert!(physics.is_active());
        }
        store.mark_chunk_loaded(chunk).unwrap();
        let resumed = physics.advance_with_context(
            TICK,
            MovementInput::default(),
            PhysicsSampleContext::default(),
            &sim::PaletteWorld::for_movement(&store, &registry, 0),
        );
        assert!(resumed.blocked.is_none(), "{:?}", resumed.blocked);
        assert_eq!(resumed.completed_ticks, 1);
        assert_eq!(physics.state().unwrap().tick, anchor.tick + 1);
    }
}

#[test]
fn flight_at_unknown_chunk_edge_resumes_after_terrain_arrival() {
    let mut registry = sim::CollisionRegistry::new();
    registry.register(0, []).unwrap();
    for (name, input) in [
        (
            "reverse",
            MovementInput {
                forward: -1.0,
                yaw_degrees: -90.0,
                ..Default::default()
            },
        ),
        (
            "ascend",
            MovementInput {
                jumping: true,
                ..Default::default()
            },
        ),
        (
            "descend",
            MovementInput {
                sneaking: true,
                ..Default::default()
            },
        ),
        ("release", MovementInput::default()),
    ] {
        let mut store = world::ChunkStore::new();
        load_air_column(&mut store, 0);
        let mut physics = reach_missing_column(&registry, &store);
        let before = physics.state().unwrap().clone();
        for _ in 0..20 {
            let frame = physics.advance_with_context(
                TICK,
                input,
                PhysicsSampleContext {
                    mode_intent: boundary_flight(),
                    ..Default::default()
                },
                &sim::PaletteWorld::new(&store, &registry, 0, Default::default()),
            );
            assert_eq!(frame.completed_ticks, 0, "{name}");
            assert_eq!(physics.state(), Some(&before), "{name}");
            assert!(physics.is_active());
            assert_eq!(
                super::super::coordination::physics_authority_fault_for_frame(&frame),
                None
            );
        }
        println!(
            "{name}: blocked at tick {}, feet {:?}, velocity {:?}",
            before.tick, before.position, before.velocity
        );
        // An unrelated vertical section cannot authorize the missing travel region.
        store
            .mark_sub_chunk_loaded(world::SubChunkKey::new(0, 1, 0, 0))
            .unwrap();
        let partial = physics.advance_with_context(
            TICK,
            input,
            PhysicsSampleContext {
                mode_intent: boundary_flight(),
                ..Default::default()
            },
            &sim::PaletteWorld::new(&store, &registry, 0, Default::default()),
        );
        assert_eq!(partial.completed_ticks, 0);
        load_air_column(&mut store, 1);
        let resumed = physics.advance_with_context(
            TICK,
            input,
            PhysicsSampleContext {
                mode_intent: boundary_flight(),
                ..Default::default()
            },
            &sim::PaletteWorld::new(&store, &registry, 0, Default::default()),
        );
        assert!(resumed.blocked.is_none(), "{name}: {:?}", resumed.blocked);
        assert_eq!(resumed.completed_ticks, 1);
        assert_eq!(physics.state().unwrap().tick, before.tick + 1);
        assert_eq!(physics.mode(), MovementMode::Flying);
        assert_eq!(resumed.dropped_ticks, 0);
    }
}

/// Samples semantic movement phases through the production frame coordinator.
fn flight_frame(
    index: u64,
    forward: f32,
    jump: bool,
    down: bool,
    active: bool,
) -> super::super::PhysicsFrameInput {
    super::super::PhysicsFrameInput {
        delta: TICK,
        now: TICK * index as u32,
        active,
        input_authority_released: !active,
        resolved_movement: false,
        movement: [0.0, forward],
        raw_movement: [0.0, forward],
        analogue_movement: [0.0, forward],
        movement_buttons: semantic_input::MovementButtons {
            forward: forward > 0.0,
            backward: forward < 0.0,
            ..Default::default()
        },
        yaw: -90.0,
        pitch: 0.0,
        camera_orientation: [1.0, 0.0, 0.0],
        input_mode: protocol::PlayerInputMode::Mouse,
        jump: semantic_input::ActionPhase {
            held: jump,
            ..Default::default()
        },
        sneak: semantic_input::ActionPhase {
            held: down,
            ..Default::default()
        },
        sprint: Default::default(),
        toggle_sprint: false,
        always_sprint: false,
        toggle_sneak: true,
        facts: super::super::local_facts::LocalMovementFacts {
            can_fly: true,
            flight: client_world::game_mode_capabilities::FlightAuthority::Mutable(true),
            creative_flight: true,
            ..Default::default()
        },
        item_use_modifier: None,
        hold: None,
    }
}

#[test]
fn loaded_flight_reversals_authority_replay_and_input_release_keep_advancing() {
    let mut registry = sim::CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let mut store = world::ChunkStore::new();
    for x in -4..=4 {
        for z in -4..=4 {
            store
                .mark_chunk_loaded(world::ChunkKey::new(0, x, z))
                .unwrap();
        }
    }
    let world = sim::PaletteWorld::new(&store, &registry, 0, Default::default());
    let position = [8.0, 80.0 + protocol::PLAYER_NETWORK_OFFSET, 8.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(position, 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 100, position);
    ticker.set_source(MovementSource::Physics);
    let mut locals = super::super::LocomotionState::default();
    let mut effects = super::super::LocalMovementEffectTimeline::default();
    let mut speed = super::super::LocalMovementSpeedAuthority::default();
    let mut index = 0;
    let mut corrections = 0;
    for (count, forward, jump, down, active) in [
        (35, 1.0, false, false, true),
        (35, -1.0, false, false, true),
        (15, 0.0, true, false, true),
        (15, 0.0, false, true, true),
        (12, 0.0, false, false, false),
        (15, 1.0, false, true, true),
        (100, 0.0, false, false, true),
    ] {
        for _ in 0..count {
            index += 1;
            let advanced = locals.advance(
                flight_frame(index, forward, jump, down, active),
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &world,
            );
            assert!(
                advanced,
                "frame {index}: {:?}, pending {}",
                ticker.pending_authority_fault(),
                ticker.pending_count()
            );
            assert_eq!(physics.state().unwrap().tick, 100 + index);
            assert_eq!(physics.mode(), MovementMode::Flying);
            assert!(ticker.can_advance_physics_frame());
            let sample = physics.sample_at(100 + index).unwrap();
            assert_eq!(sample.move_vector, [0.0, forward]);
            assert_eq!(sample.jumping, jump);
            assert_eq!(sample.sneaking, down);
            let mut admitted = Vec::new();
            super::super::flush_player_auth_inputs(
                &mut ticker,
                8,
                Some(super::super::integration_tests::evidence_context()),
                |identity, _packet| {
                    admitted.push(identity);
                    Ok::<_, &str>(())
                },
            )
            .unwrap();
            for identity in admitted {
                assert!(ticker.acknowledge_physics_send(identity));
            }
            let _ = ticker.take_tick_evidence();
            if index % 25 == 0 {
                let before = physics.state().unwrap().clone();
                let old = physics.sample_at(before.tick - 4).unwrap().clone();
                let mut corrected_position = old.position;
                corrected_position[0] += 0.125;
                reconcile_candidate_physics_correction(
                    &mut ticker,
                    &mut physics,
                    corrected_position,
                    old.tick,
                    false,
                    super::super::PhysicsCorrectionMode::ReplayIfRetained,
                    &world,
                )
                .unwrap();
                corrections += 1;
                assert_eq!(physics.state().unwrap().tick, before.tick);
                assert!(
                    (physics.state().unwrap().position.x - before.position.x - 0.125).abs()
                        < 0.00002
                );
                assert_eq!(physics.state().unwrap().velocity, before.velocity);
                assert_eq!(physics.mode(), MovementMode::Flying);
            }
            physics.apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    sneaking: Some(false),
                    ..Default::default()
                },
            );
        }
    }
    let velocity = physics.state().unwrap().velocity;
    assert!(velocity.length_squared() < 0.000001);
    assert_eq!(ticker.pending_count(), 0);
    assert!(ticker.physics_is_authorized());
    println!(
        "loaded real-frame flight: {index} contiguous ticks, {corrections} delayed corrections; released velocity {velocity:?}"
    );
}

/// Missing neighbouring terrain must not prevent escape into the loaded column.
#[test]
fn flight_can_reverse_away_from_an_unloaded_column() {
    let mut registry = sim::CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let mut store = world::ChunkStore::new();
    load_air_column(&mut store, 0);
    let mut physics = reach_missing_column(&registry, &store);
    let before = physics.state().unwrap().clone();
    let world = sim::PaletteWorld::for_movement(&store, &registry, 0);
    let reverse = MovementInput {
        forward: -1.0,
        yaw_degrees: -90.0,
        ..Default::default()
    };
    for _ in 0..20 {
        physics.advance_with_context(
            TICK,
            reverse,
            PhysicsSampleContext {
                mode_intent: boundary_flight(),
                ..Default::default()
            },
            &world,
        );
    }
    assert!(
        physics.state().unwrap().tick > before.tick,
        "reverse input never completed a tick beside unknown terrain"
    );
    assert!(
        physics.state().unwrap().position.x < before.position.x,
        "reverse input must return into the loaded column"
    );
    assert_eq!(physics.mode(), MovementMode::Flying);
}

/// Retained boundary collision survives replay even after the live terrain arrives.
#[test]
fn flight_boundary_replay_preserves_then_releases_the_streaming_edge() {
    let mut registry = sim::CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let mut store = world::ChunkStore::new();
    load_air_column(&mut store, 0);
    let mut physics = reach_missing_column(&registry, &store);
    let input = MovementInput {
        forward: 1.0,
        yaw_degrees: -90.0,
        ..Default::default()
    };
    let mut samples = Vec::new();
    for _ in 0..12 {
        samples.push(step(
            &mut physics,
            input,
            boundary_flight(),
            &sim::PaletteWorld::for_movement(&store, &registry, 0),
        ));
    }
    let expected = physics.state().unwrap().clone();
    assert!(
        sim::Aabb::player_at(expected.position).max.x <= world::SUB_CHUNK_SIDE as f64 + 0.00001
    );
    assert_eq!(expected.velocity.x, 0.0);
    let old = samples[7].clone();
    load_air_column(&mut store, 1);
    physics
        .apply_correction(
            super::super::PhysicsAnchor {
                network_position: old.position,
                tick: old.tick,
                on_ground: old.grounded_after_tick,
                velocity: Some(old.velocity),
            },
            super::super::PhysicsCorrectionMode::ReplayIfRetained,
            None,
            &sim::PaletteWorld::for_movement(&store, &registry, 0),
        )
        .unwrap();
    assert_eq!(physics.state().unwrap().position, expected.position);
    assert_eq!(physics.state().unwrap().velocity, expected.velocity);
    let resumed = step(
        &mut physics,
        input,
        boundary_flight(),
        &sim::PaletteWorld::for_movement(&store, &registry, 0),
    );
    assert!(resumed.movement[0] > 0.0);
    assert_eq!(resumed.processed.mode, MovementMode::Flying);
}

/// A horizontal streaming boundary does not suppress independent vertical or release controls.
#[test]
fn flight_boundary_keeps_vertical_controls_and_release_responsive() {
    let mut registry = sim::CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let mut store = world::ChunkStore::new();
    load_air_column(&mut store, 0);
    let world = sim::PaletteWorld::for_movement(&store, &registry, 0);
    for input in [
        MovementInput {
            jumping: true,
            ..Default::default()
        },
        MovementInput {
            sneaking: true,
            ..Default::default()
        },
        MovementInput::default(),
    ] {
        let mut physics = reach_missing_column(&registry, &store);
        let before = physics.state().unwrap().clone();
        let mut last = None;
        for _ in 0..20 {
            last = Some(step(&mut physics, input, boundary_flight(), &world));
            physics.apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    sneaking: Some(false),
                    ..Default::default()
                },
            );
        }
        let after = physics.state().unwrap();
        assert_eq!(after.tick, before.tick + 20);
        assert!(
            sim::Aabb::player_at(after.position).max.x <= world::SUB_CHUNK_SIDE as f64 + 0.00001
        );
        assert_eq!(physics.mode(), MovementMode::Flying);
        if input.jumping {
            assert!(after.position.y > before.position.y);
        } else if input.sneaking {
            assert!(after.position.y < before.position.y);
        } else {
            assert_eq!(after.position.y, before.position.y);
            assert_eq!(after.velocity.x, 0.0);
        }
        let flags = input_flags(last.as_ref().unwrap(), HeldInput::default());
        assert_eq!(has(flags, PlayerInputFlags::WANT_UP), input.jumping);
        assert_eq!(has(flags, PlayerInputFlags::WANT_DOWN), input.sneaking);
    }
}

/// Turning flight off and on at the edge retains responsive movement and wire transitions.
#[test]
fn flight_toggles_at_unloaded_boundary_keep_ticks_and_edges() {
    let mut registry = sim::CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let mut store = world::ChunkStore::new();
    load_air_column(&mut store, 0);
    let world = sim::PaletteWorld::for_movement(&store, &registry, 0);
    let mut physics = reach_missing_column(&registry, &store);
    for _ in 0..12 {
        step(
            &mut physics,
            MovementInput::default(),
            boundary_flight(),
            &world,
        );
    }
    let edge = physics.state().unwrap().position;
    let off = double_tap_jump(&mut physics, boundary_flight(), &world);
    assert_eq!(off.processed.mode, MovementMode::Walking);
    assert!(has(
        input_flags(
            &off,
            HeldInput::from(physics.sample_at(off.tick - 1).unwrap())
        ),
        PlayerInputFlags::STOP_FLYING
    ));
    for _ in 0..10 {
        step(
            &mut physics,
            MovementInput {
                forward: -1.0,
                yaw_degrees: -90.0,
                ..Default::default()
            },
            boundary_flight(),
            &world,
        );
    }
    assert!(physics.state().unwrap().position.x < edge.x);
    let on = double_tap_jump(&mut physics, boundary_flight(), &world);
    assert_eq!(on.processed.mode, MovementMode::Flying);
    assert!(has(
        input_flags(
            &on,
            HeldInput::from(physics.sample_at(on.tick - 1).unwrap())
        ),
        PlayerInputFlags::START_FLYING
    ));
    let tick = physics.state().unwrap().tick;
    for _ in 0..10 {
        step(
            &mut physics,
            MovementInput {
                jumping: true,
                ..Default::default()
            },
            boundary_flight(),
            &world,
        );
    }
    assert_eq!(physics.state().unwrap().tick, tick + 10);
    assert_eq!(physics.mode(), MovementMode::Flying);
}

#[path = "ability_axes_tests.rs"]
mod ability_axes_tests;
