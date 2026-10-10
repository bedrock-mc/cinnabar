use super::*;

/// Builds a one-block-high ledge whose forward ray misses while its downward ray hits.
fn ledge_fixture(pitch: f32) -> (World, client_session::CapturedPackets) {
    let start = [4.5, 2.620_01, 6.1];
    let rotation = Quat::from_rotation_x(pitch);
    let (mut world, captured) = floor_fixture(start, rotation);
    let air = assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
    .iter()
    .find(|record| record.name.as_ref() == "minecraft:air")
    .unwrap()
    .sequential_id;
    {
        let mut client = world.resource_mut::<crate::runtime::world::ClientWorld>();
        let stream = client.stream.as_mut().unwrap();
        stream
            .submit(
                4,
                protocol::WorldEvent::BlockUpdates(vec![protocol::BlockUpdateEvent {
                    dimension: 0,
                    position: [4, 2, 6],
                    layer: 0,
                    network_id: air,
                }]),
            )
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while stream.committed_sequence() < 4 {
            stream.poll(start, 0);
            assert!(
                Instant::now() < deadline,
                "ledge fixture decode did not finish"
            );
            std::thread::yield_now();
        }
    }
    let origin = frame_origin(&world, Vec3::from_array(start), rotation);
    let authority = world
        .resource::<MovementTicker>()
        .interaction_authority_identity();
    world
        .resource_mut::<BlockUseRuntime>()
        .retain_pick(&origin, authority);
    world.insert_resource(origin);
    (world, captured)
}

#[test]
fn looking_past_a_ledge_places_on_the_forward_face_of_support() {
    let (mut world, mut captured) = ledge_fixture(-0.9601);
    world.run_system_cached(produce_block_use).unwrap();
    use protocol::wire::valentine::bedrock::version::v1_26_51::{
        InventoryTransactionPacketTransaction, McpePacketData,
    };
    let transaction = captured
        .drain()
        .into_iter()
        .find_map(|packet| {
            let McpePacketData::InventoryTransactionPacket(packet) = packet.data else {
                return None;
            };
            let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(tx) =
                packet.transaction
            else {
                return None;
            };
            Some(tx)
        })
        .expect("forward placement transaction");
    assert_eq!(
        [
            transaction.position.x,
            transaction.position.y,
            transaction.position.z
        ],
        [4, 0, 6]
    );
    assert_eq!(transaction.face, 2);
    assert_eq!(transaction.click_position.x, 0.5);
    assert_eq!(transaction.click_position.y, 1.0);
    assert!((transaction.click_position.z - 0.1).abs() < 1.0e-5);
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let stream = client.stream.as_ref().unwrap();
    let palette = sim::PaletteWorld::new(
        stream.collision_store(),
        world
            .resource::<PhysicsCollisionRegistries>()
            .registry(stream.network_id_mode()),
        stream.current_dimension(),
    );
    assert_eq!(
        palette.primary_runtime_id([4, 0, 5]).unwrap(),
        transaction.target_block_id
    );
}

#[test]
fn shallow_ledge_miss_does_not_place() {
    let (mut world, mut captured) = ledge_fixture(-0.7);
    world.run_system_cached(produce_block_use).unwrap();
    assert!(transaction_targets(&mut captured).is_empty());
}

#[test]
fn ledge_miss_outlines_the_supporting_block() {
    let (mut world, _) = ledge_fixture(-0.9601);
    world.init_resource::<crate::settings_runtime::RuntimeSettings>();
    let bounds = published_outline(&mut world).expect("ledge support outline");
    // The supporting block [4, 0, 6], outlined as a full unit box.
    assert_eq!(bounds, [[4.0, 0.0, 6.0], [5.0, 1.0, 7.0]]);
}

#[test]
fn spectator_ledge_miss_has_no_use_or_outline() {
    let (mut world, mut captured) = ledge_fixture(-0.9601);
    world
        .resource_mut::<crate::player_runtime::PlayerRuntime>()
        .facts
        .publish_player_game_mode(PlayerGameMode::Spectator);
    world.init_resource::<crate::settings_runtime::RuntimeSettings>();
    assert!(published_outline(&mut world).is_none());
    world.run_system_cached(produce_block_use).unwrap();
    assert!(transaction_targets(&mut captured).is_empty());
}

#[test]
fn a_direct_floor_hit_wins_over_indirect_support() {
    let (mut world, mut captured) =
        floor_fixture([4.5, 2.620_01, 8.32], Quat::from_rotation_x(-0.9601));
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 0, 7]]);
}

#[test]
fn a_picked_actor_prevents_indirect_support_use_and_outline() {
    let (mut world, mut captured) = ledge_fixture(-0.9601);
    actor_use_tests::spawn(&mut world, 5.4);
    let origin = frame_origin(
        &world,
        Vec3::new(4.5, 2.620_01, 6.1),
        Quat::from_rotation_x(-0.9601),
    );
    let authority = world
        .resource::<MovementTicker>()
        .interaction_authority_identity();
    world
        .resource_mut::<BlockUseRuntime>()
        .retain_pick(&origin, authority);
    world.insert_resource(origin);
    world.init_resource::<crate::settings_runtime::RuntimeSettings>();
    assert!(published_outline(&mut world).is_none());
    world.run_system_cached(produce_block_use).unwrap();
    assert!(transaction_targets(&mut captured).is_empty());
    assert!(world.resource::<BlockUseRuntime>().press_interacted());
}

#[test]
fn sneaking_and_airborne_states_can_use_nearby_ledge_support() {
    for sneaking in [false, true] {
        let (mut world, mut captured) = ledge_fixture(-0.9601);
        let mut sample = gameplay::test_support::survival_mining::completed(101);
        sample.position = [4.5, 2.72, 6.1];
        sample.sneaking = sneaking;
        sample.grounded_before_tick = false;
        sample.grounded_after_tick = false;
        world
            .resource_mut::<MovementTicker>()
            .enqueue_completed_physics(sample)
            .unwrap();
        world.run_system_cached(produce_block_use).unwrap();
        assert_eq!(transaction_targets(&mut captured), [[4, 0, 6]]);
    }
}

/// Resolves the production target for one action without changing terrain or hold history.
fn observed_target(world: &mut World, trigger: ItemUseTrigger) -> Option<FrozenBlockObservation> {
    let mut system = bevy::ecs::system::SystemState::<(
        BlockUseContext,
        Res<crate::player_runtime::PlayerRuntime>,
        Res<BlockUseRuntime>,
        Res<MovementTicker>,
    )>::new(world);
    let (context, player, runtime, movement) = system.get_mut(world);
    let input = context.input.snapshot()?;
    let authority = movement.interaction_authority_identity();
    observe_use_target(
        &player,
        &context,
        protocol_input_mode(input.input_mode),
        true,
        (input.authority_generation, input.frame_sequence),
        authority.1,
        &runtime,
        runtime.pick(authority),
        trigger,
        &movement.build_action_state()?,
    )
}

#[test]
fn indirect_support_starts_use_but_does_not_repeat_without_a_line() {
    let (mut world, _) = ledge_fixture(-0.9601);
    assert!(observed_target(&mut world, ItemUseTrigger::PlayerInput).is_some());
    world.resource_mut::<BlockUseRuntime>().intention.record(
        false,
        [4, 0, 5],
        LocalUse::Place,
        true,
        false,
        [4.5, 1.0, 6.1],
    );
    assert!(observed_target(&mut world, ItemUseTrigger::SimulationTick).is_none());
}

#[test]
fn a_locked_line_uses_the_indirect_intercept_instead_of_the_forward_miss_segment() {
    let (mut world, _) = ledge_fixture(-0.9601);
    {
        let mut runtime = world.resource_mut::<BlockUseRuntime>();
        for (repeated, destination) in [(false, [4, 0, 6]), (true, [4, 0, 5])] {
            runtime.intention.record(
                repeated,
                destination,
                LocalUse::Place,
                true,
                false,
                [4.5, 1.0, 6.1],
            );
        }
    }
    let ray = world
        .resource::<InteractionOriginSnapshot>()
        .outbound_ray()
        .unwrap();
    let endpoint = ray.origin() + ray.direction() * survival_reach(PlayerInputMode::Mouse) as f32;
    assert!(
        world
            .resource::<BlockUseRuntime>()
            .intention
            .target(
                None,
                ray.origin().to_array(),
                endpoint.to_array(),
                [0.0; 3],
                false,
            )
            .is_some(),
        "the forward miss segment reaches the next line cell"
    );
    assert!(
        observed_target(&mut world, ItemUseTrigger::SimulationTick).is_none(),
        "the downward support intercept does not reach that cell"
    );
}

/// Corners of the outline the production selection system publishes, if any.
fn published_outline(world: &mut World) -> Option<[[f32; 3]; 2]> {
    world.init_resource::<render::BlockSelectionFrame>();
    world
        .run_system_cached(crate::block_selection::publish)
        .unwrap();
    let frame = world.resource::<render::BlockSelectionFrame>();
    let mut points = frame
        .outline
        .iter()
        .map(|vertex| Vec3::from_array(vertex.position));
    let first = points.next()?;
    let (min, max) = points.fold((first, first), |(min, max), point| {
        (min.min(point), max.max(point))
    });
    Some([min.to_array(), max.to_array()])
}
