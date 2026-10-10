use {
    super::*,
    client_presentation::actor_publication::{HandRigBuilder, publish_actor_render_frame},
};

#[test]
fn offline_actor_frames_publish_and_advance_the_synthetic_actor() {
    let (_pack, artwork, entities) = crate::tests::actor_rest_presentation::compiled_fixture(
        "1.0",
        1,
        assets::ActorPoseMode::CompiledLiteral,
    );
    let mut stream = crate::tests::actor_rest_presentation::stream(Arc::clone(&entities));
    stream
        .submit(
            1,
            WorldEvent::Actor(protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: 42,
                runtime_id: 42,
                kind: ActorKind::Entity {
                    identifier: "minecraft:example".into(),
                },
                position: [0.0, 64.0, 0.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    assert!(
        stream.authority().actor_rig(42).is_some(),
        "the synthetic actor is admitted before preparation"
    );
    let mut scene = render::ActorRenderScene::with_runtime_entity_assets(&entities).unwrap();
    scene.configure_artwork(artwork.clone());
    let mut client = crate::runtime::world::ClientWorld::new_with_entity_assets(
        Arc::new(assets::RuntimeAssets::diagnostic()),
        Arc::clone(&entities),
    );
    client.stream = Some(stream);
    let mut world = crate::tests::actor_frame_allocations::actor_frame_world(
        client,
        scene,
        artwork,
        HandRigBuilder::from_runtime_assets(&entities).unwrap(),
        (Vec3::new(0.0, 66.0, -12.0), Vec3::new(0.0, 64.0, 4.0)),
    );
    let clock = Instant::now();
    world
        .resource_mut::<Time<Real>>()
        .update_with_instant(clock);
    prepare_offline_actor_frame(&mut world);
    world.run_system_cached(publish_actor_render_frame).unwrap();
    let (before_tick, before_pose) = {
        let client = world.resource::<crate::runtime::world::ClientWorld>();
        let rig = client
            .stream
            .as_ref()
            .unwrap()
            .authority()
            .actor_rig(42)
            .unwrap();
        (rig.completed_tick, rig.current.to_vec())
    };
    world
        .resource_mut::<Time<Real>>()
        .update_with_instant(clock + world::TICK_DURATION);
    prepare_offline_actor_frame(&mut world);
    world.run_system_cached(publish_actor_render_frame).unwrap();
    assert_eq!(
        world.resource::<ActorRenderFrame>().rig.instances.len(),
        1,
        "offline reports must publish their submitted actor"
    );
    let client = world.resource::<crate::runtime::world::ClientWorld>();
    let rig = client
        .stream
        .as_ref()
        .unwrap()
        .authority()
        .actor_rig(42)
        .unwrap();
    assert_eq!(
        rig.completed_tick,
        before_tick + 1,
        "each report frame advances actors once"
    );
    assert_ne!(
        rig.current, before_pose,
        "the report evaluates the actor's animated pose"
    );
}
