//! Ordinary trident release through production item selection and session admission.

use super::*;
use client_interaction::movement::LocalPhysicsController;
use protocol_ids::Enchantment;

#[path = "riptide/sprint.rs"]
mod sprint;

#[path = "riptide/respawn.rs"]
mod respawn;

#[path = "riptide/backpressure.rs"]
mod backpressure;

#[path = "riptide/ordered.rs"]
mod ordered;

/// Water everywhere keeps item eligibility independent of fixture terrain and rain.
struct Water;

impl sim::CollisionWorld for Water {
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, _: [i32; 3]) -> Result<sim::BlockPhysicsSample, sim::WorldQueryError> {
        Ok(sim::BlockPhysicsSample {
            layers: Box::new([sim::BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 1.0,
                flags: sim::BlockPhysicsFlags::WATER,
                surface_response: sim::SurfaceResponse::None,
            }]),
            identity: sim::CollisionQuery::synthetic(()).identity,
        })
    }
}

/// Encodes the stored enchantment list read by production item selection.
fn riptide_data() -> std::sync::Arc<[u8]> {
    let mut bytes = vec![255, 255, 1, 10, 0, 0, 9, 4, 0];
    bytes.extend(b"ench");
    bytes.push(10);
    bytes.extend(1_i32.to_le_bytes());
    for (name, value) in [
        (
            b"id".as_slice(),
            i16::from(Enchantment::Riptide.wire_value()),
        ),
        (b"lvl".as_slice(), 3),
    ] {
        bytes.push(2);
        bytes.extend((name.len() as u16).to_le_bytes());
        bytes.extend(name);
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0, 0]);
    bytes.into()
}

/// Advances the production physics owner once and publishes its completed movement input.
fn tick(world: &mut World) -> gameplay::movement::PhysicsMovementSample {
    let sample = world
        .resource_mut::<LocalPhysicsController>()
        .advance(
            simulation_clock::TICK_DURATION,
            sim::MovementInput::default(),
            &Water,
        )
        .samples
        .into_iter()
        .next()
        .unwrap();
    world
        .resource_mut::<MovementTicker>()
        .enqueue_completed_physics(sample.clone())
        .unwrap();
    sample
}

/// Mirrors the socket receipts that allow a release to follow its facing input.
fn flush_and_acknowledge(world: &mut World) {
    world.resource_scope(|world, mut movement: Mut<MovementTicker>| {
        let mut written = Vec::new();
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut movement,
            gameplay::movement::OUTBOX_CAPACITY,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| {
                written.push(identity);
                world
                    .resource::<NetworkHandle<PackApplication>>()
                    .send_physics_packet(identity, packet, guard)
            },
        )
        .unwrap();
        for identity in written {
            assert!(movement.acknowledge_physics_send(identity));
        }
    });
}

/// Exercises the ordinary producer with an empty outbox at the physical release edge.
fn ordinary_release(charge_ticks: u64) {
    let (mut world, mut captured) = fixture();
    item_use_tests::hold_air_item(&mut world, "minecraft:trident", 1, riptide_data());
    let position = world
        .resource::<MovementTicker>()
        .newest_unsent_sample()
        .unwrap()
        .position;
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(position, 100, false);
    physics.advance(
        simulation_clock::TICK_DURATION,
        sim::MovementInput::default(),
        &Water,
    );
    assert!(physics.in_water());
    world.insert_resource(physics);
    flush_and_acknowledge(&mut world);
    produce_air_use(&mut world);
    assert!(
        world
            .resource::<client_interaction::item_use::ItemUseRuntime>()
            .is_using()
    );
    captured.drain();
    for _ in 0..charge_ticks {
        tick(&mut world);
        flush_and_acknowledge(&mut world);
        produce_air_use(&mut world);
        captured.drain();
    }
    assert!(
        world
            .resource::<MovementTicker>()
            .newest_unsent_sample()
            .is_none()
    );
    assert!(
        world
            .resource::<MovementTicker>()
            .between_ticks_sample()
            .is_some()
    );
    let rotation = Quat::from_euler(EulerRot::YXZ, 90_f32.to_radians(), 30_f32.to_radians(), 0.0);
    world.resource_mut::<LocalViewPose>().set_rotation(rotation);
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame::default()),
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    let slowdown = world
        .resource::<client_interaction::item_use::ItemUseRuntime>()
        .movement_modifier()
        .unwrap();
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                keys: vec![0x1a],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    produce_air_use(&mut world);
    assert!(
        captured.drain().is_empty(),
        "ordinary release must wait for its release-facing input"
    );
    assert!(world.resource::<MovementTicker>().has_held_release());
    // A later look must affect ordinary input without redirecting the admitted action.
    world
        .resource_mut::<LocalViewPose>()
        .set_rotation(Quat::IDENTITY);
    let mut expected = world.resource::<LocalPhysicsController>().clone();
    let expected = expected
        .advance(
            simulation_clock::TICK_DURATION,
            gameplay::movement::physics_movement_input(
                [0.0, 1.0],
                180.0,
                true,
                false,
                false,
                false,
                Some(slowdown),
            ),
            &Water,
        )
        .samples
        .remove(0);
    world.insert_resource(client_presentation::camera::AutoFly::new(false));
    #[cfg(feature = "acceptance")]
    world.insert_resource(AcceptanceRun::new(None, None, false, false));
    #[cfg(not(feature = "acceptance"))]
    world.init_resource::<AcceptanceRun>();
    world.init_resource::<client_interaction::movement::LocalMovementSpeedAuthority>();
    world.init_resource::<client_interaction::movement::LocomotionState>();
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION);
    world
        .run_system_cached(crate::movement::advance_local_physics)
        .unwrap();
    let facing = world
        .resource::<LocalPhysicsController>()
        .sample_at(expected.tick)
        .unwrap();
    assert_eq!(
        facing.move_vector, expected.move_vector,
        "the release-facing tick must retain item-use slowdown"
    );
    assert!(
        !facing.processed.spin_started,
        "the facing input must precede the launch"
    );
    flush_and_acknowledge(&mut world);
    produce_air_use(&mut world);
    let packets = captured.drain();
    assert_eq!(packets.len(), 2, "one facing input then one release");
    let facing = protocol::player_auth_input_trace_sample(&packets[0]).unwrap();
    assert!((facing.pitch + 30.0).abs() < 1e-3);
    assert!((facing.yaw - 90.0).abs() < 1e-3);
    assert!(!facing.flag_names.contains(&"StartSpinAttack"));
    assert!(protocol::is_aim_assist_rotation_action(&packets[1]));
    let launch = tick(&mut world);
    assert_eq!(
        launch.processed.spin_started,
        charge_ticks >= gameplay::item_use::RIPTIDE_CHARGE_TICKS
    );
    assert_eq!(
        launch.movement[0] < -1.0,
        charge_ticks >= gameplay::item_use::RIPTIDE_CHARGE_TICKS
    );
    flush_and_acknowledge(&mut world);
    produce_air_use(&mut world);
    assert_eq!(captured.drain().len(), 1);
    assert!(
        !tick(&mut world).processed.spin_started,
        "a release launches only once"
    );
    flush_and_acknowledge(&mut world);
    captured.drain();
    for _ in 0..3 {
        produce_air_use(&mut world);
    }
    assert!(
        captured.drain().is_empty(),
        "a release transaction is sent only once"
    );
}

#[test]
fn ordinary_riptide_release_keeps_its_facing_and_launch_order() {
    ordinary_release(gameplay::item_use::RIPTIDE_CHARGE_TICKS);
}

#[test]
fn early_ordinary_riptide_release_does_not_gain_charge_while_fenced() {
    ordinary_release(gameplay::item_use::RIPTIDE_CHARGE_TICKS - 1);
}
