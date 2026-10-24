use super::*;
use client_interaction::item_use::ItemUseRuntime;
use gameplay::item_use::UseFrame;

struct EmptyWorld;

impl sim::CollisionWorld for EmptyWorld {
    /// Supplies empty terrain for retained movement captures.
    fn collision_boxes(
        &self,
        _: sim::Aabb,
    ) -> Result<sim::CollisionQuery<Vec<sim::Aabb>>, sim::WorldQueryError> {
        Ok(sim::CollisionQuery::synthetic(Vec::new()))
    }
}

/// Supplies a verified bow and held-use facts without depending on installed assets.
fn use_frame(tick: u64, held: bool) -> UseFrame {
    let stack = protocol::NetworkItemStack {
        network_id: 300,
        stack_network_id: 1,
        count: 1,
        ..Default::default()
    };
    UseFrame {
        riptide: None,
        riptide_facing: Default::default(),
        tick,
        now_millis: tick * simulation_clock::TICK_DURATION.as_millis() as u64,
        position: [0.0, 70.0, 0.0],
        held,
        selection: Some(gameplay::mining::FrozenMiningSelection {
            slot: 0,
            item: protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest)
                .unwrap(),
        }),
        air_use: gameplay::item_use::fixtures::classify("minecraft:bow", false, 0, None),
        ready: true,
        creative: false,
        inventory_revision: Some(1),
        charge_projectile: None,
        press_consumed: false,
    }
}

/// Sends a retained echo through the app's real world-control consumer, then reconciles use.
fn check_stamped_echo(current_use: bool) {
    let (mut app, _) = fixture_app();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 70.0, 0.0], 100, false);
    physics.write_local_using_item(!current_use);
    for _ in 0..2 {
        let samples = physics
            .advance(
                simulation_clock::TICK_DURATION,
                sim::MovementInput::default(),
                &EmptyWorld,
            )
            .samples;
        assert_eq!(samples.len(), 1);
    }
    physics.write_local_using_item(current_use);
    app.insert_resource(physics);
    {
        let mut movement = app.world_mut().resource_mut::<MovementTicker>();
        movement.reset(1, 102, [0.0, 70.0, 0.0]);
        movement.set_source(gameplay::movement::MovementSource::Physics);
    }
    let mut runtime = ItemUseRuntime::default();
    runtime.synchronize(1);
    runtime.observe_press(true);
    assert!(runtime.step(&use_frame(102, true)).started);
    if !current_use {
        assert!(!runtime.step(&use_frame(103, false)).packets.is_empty());
    }
    runtime.take_local_using_item();
    let timing = runtime.active_timing();
    app.insert_resource(runtime);
    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            1,
            WorldEvent::Actor(protocol::ActorEvent::Metadata(
                protocol::ActorMetadataUpdateEvent {
                    dimension: 0,
                    runtime_id: 42,
                    tick: 101,
                    metadata: Arc::from([protocol::ActorMetadata {
                        key: protocol_ids::ActorDataKey::Flags.wire_value(),
                        value: protocol::ActorMetadataValue::Flags(
                            (1 << protocol_ids::ActorFlag::HasGravity.wire_value())
                                | (1 << protocol_ids::ActorFlag::HasCollision.wire_value())
                                | if current_use {
                                    0
                                } else {
                                    1 << protocol_ids::ActorFlag::UsingItem.wire_value()
                                },
                        ),
                    }]),
                    properties: Arc::from([]),
                },
            )),
        )
        .unwrap();
    app.update();
    assert!(app.world().resource::<FatalError>().0.is_none());
    let stream = app
        .world()
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .unwrap();
    assert_eq!(stream.committed_sequence(), 1);
    assert_eq!(
        stream.authority().actor(42).unwrap().is_using_item(),
        !current_use,
        "metadata retains the raw server value"
    );
    let accepted = app
        .world_mut()
        .resource_mut::<LocalPhysicsController>()
        .take_server_using_item();
    assert_eq!(accepted, None, "the echo matches its historical capture");
    let mut runtime = app.world_mut().resource_mut::<ItemUseRuntime>();
    runtime.adopt_server_using_item(accepted);
    runtime.resolve_server_using_item(&use_frame(104, current_use));
    assert_eq!(runtime.is_using(), current_use);
    assert_eq!(runtime.active_timing(), timing);
    assert_eq!(runtime.reported_using_item(), Some(current_use));
    assert!(!runtime.has_server_using_item());
}

#[test]
fn stamped_echo_does_not_restart_a_released_use() {
    check_stamped_echo(false);
}

#[test]
fn stamped_echo_does_not_stop_a_newer_charge() {
    check_stamped_echo(true);
}
