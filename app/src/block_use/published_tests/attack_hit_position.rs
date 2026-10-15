//! Attack packets retain the actor intercept published before the press frame.
use super::*;
use protocol::test_support::transactions::observe_transaction;

/// Supplies one selectable actor and drains movement before a frame-time attack.
pub(super) fn attack_fixture(z: f32) -> (World, client_session::CapturedPackets) {
    let (mut world, mut captured) = fixture();
    actor_use_tests::spawn(&mut world, z);
    world.init_resource::<crate::melee::MeleeTarget>();
    world.init_resource::<client_presentation::aim_assist::AimAssistFrame>();
    world.init_resource::<client_presentation::camera::ServerCameraView>();
    world.resource_scope(|world, mut movement: Mut<MovementTicker>| {
        let network = world.resource::<NetworkHandle<PackApplication>>();
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut movement,
            gameplay::movement::OUTBOX_CAPACITY,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard| network.send_physics_packet(identity, packet, guard),
        )
        .unwrap();
    });
    captured.drain();
    // Publish the frame before the attack without pressing the attack button.
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    (world, captured)
}

/// Admits a fresh attack through the production pick and packet producers.
pub(super) fn attack(world: &mut World) {
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: vec![1],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world
        .run_system_cached(crate::melee::produce_melee)
        .unwrap();
}

/// Reads the ordinary actor ray without changing its frame or interaction authority.
fn picked_point(world: &World) -> [f32; 3] {
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let authority = client.stream.as_ref().unwrap().authority();
    let ray = world
        .resource::<InteractionOriginSnapshot>()
        .outbound_ray()
        .unwrap();
    gameplay::melee::pick_actor_by(
        authority.remote_actors(),
        |actor| authority.pick_hit_boxes(actor),
        None,
        ray.origin().to_array(),
        ray.direction().to_array(),
        3.0,
    )
    .unwrap()
    .point
}

/// Applies one ordered actor movement and advances the requested interpolation steps.
fn move_actor(world: &mut World, position: [Option<f32>; 3], ticks: u32) {
    let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_mut().unwrap();
    let sequence = stream.committed_sequence() + 1;
    stream
        .submit(
            sequence,
            protocol::WorldEvent::Actor(protocol::ActorEvent::Move(protocol::ActorMoveEvent {
                dimension: 0,
                runtime_id: 99,
                position,
                position_origin: protocol::ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: None,
                teleported: false,
                player_mode: None,
                source_tick: None,
                interpolation: Default::default(),
            })),
        )
        .unwrap();
    stream.poll([4.5, 2.620_01, 8.5], 0);
    assert_eq!(stream.committed_sequence(), sequence);
    stream.advance_actor_interpolation_frame(ticks);
}

#[test]
fn attack_hit_position_keeps_the_previous_frame_intercept_when_the_actor_moves() {
    let (mut world, mut captured) = attack_fixture(7.5);
    let previous = picked_point(&world);
    move_actor(&mut world, [None, None, Some(7.8)], 1);
    let current = picked_point(&world);
    assert_ne!(previous, current, "the actor must move before this press");
    attack(&mut world);
    let packets = captured.drain();
    let tx = packets.iter().find_map(observe_transaction).unwrap();
    let actor = tx.actor_use().unwrap();
    assert_eq!(actor.runtime_id, 99);
    assert_eq!(actor.hit_position, previous);
    assert_eq!(actor.from_position, [4.5, 2.620_01, 8.5]);
}

#[test]
fn attack_hit_position_uses_the_published_ray_when_the_press_frame_looks_away() {
    let (mut world, mut captured) = attack_fixture(7.5);
    let previous = picked_point(&world);
    let ray = frame_origin(
        &world,
        Vec3::new(4.5, 2.5, 8.5),
        Quat::from_rotation_y(std::f32::consts::PI),
    );
    world.insert_resource(ray);
    attack(&mut world);
    let packets = captured.drain();
    let tx = packets
        .iter()
        .find_map(observe_transaction)
        .expect("the published actor hit survives the new look direction");
    assert_eq!(tx.actor_use().unwrap().hit_position, previous);
}

#[test]
fn attack_hit_position_keeps_the_ray_origin_for_an_enclosing_box() {
    let (mut world, mut captured) = attack_fixture(8.5);
    let previous = picked_point(&world);
    assert_eq!(previous, [4.5, 2.5, 8.5]);
    let ray = frame_origin(&world, Vec3::new(4.6, 2.5, 8.5), Quat::IDENTITY);
    world.insert_resource(ray);
    attack(&mut world);
    let packets = captured.drain();
    let tx = packets.iter().find_map(observe_transaction).unwrap();
    assert_eq!(tx.actor_use().unwrap().hit_position, previous);
}

#[test]
fn attack_hit_position_does_not_retarget_a_reused_runtime_id() {
    let (mut world, mut captured) = attack_fixture(7.5);
    actor_use_tests::spawn(&mut world, 7.8);
    attack(&mut world);
    assert!(
        captured.drain().iter().all(|packet| {
            observe_transaction(packet).is_none_or(|tx| tx.actor_use().is_none())
        })
    );
}

#[test]
fn attack_hit_position_retires_the_pick_after_a_position_reset() {
    let (mut world, mut captured) = attack_fixture(7.5);
    {
        let mut movement = world.resource_mut::<MovementTicker>();
        movement.reset(7, 101, [4.5, 2.620_01, 8.5]);
        movement.set_source(gameplay::movement::MovementSource::Physics);
    }
    attack(&mut world);
    assert!(captured.drain().is_empty());
}

#[test]
fn attack_hit_position_keeps_the_assisted_actor_point_instead_of_the_box_entry() {
    for moved_off_ray in [false, true] {
        let (mut world, mut captured) = attack_fixture(7.5);
        let point = Vec3::new(4.5, 2.4, 7.5);
        let direction = (point - Vec3::new(4.5, 2.5, 8.5)).normalize();
        world
            .resource_mut::<client_presentation::aim_assist::AimAssistFrame>()
            .target = Some(client_presentation::aim_assist::AimAssistTarget {
            kind: client_presentation::aim_assist::TargetKind::Actor(99),
            point,
            direction,
            score: 0.0,
        });
        world
            .resource_mut::<InteractionOriginSnapshot>()
            .assist_direction(direction);
        assert_ne!(picked_point(&world), point.to_array());
        if moved_off_ray {
            move_actor(&mut world, [Some(6.5), None, None], 3);
        }
        world
            .run_system_cached(crate::melee::prepare_melee)
            .unwrap();
        attack(&mut world);
        let packets = captured.drain();
        let tx = packets.iter().find_map(observe_transaction).unwrap();
        let actor = tx.actor_use().unwrap();
        assert_eq!(actor.runtime_id, 99);
        assert_eq!(actor.hit_position, point.to_array());
    }
}
