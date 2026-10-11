use super::*;

/// Creates an ordinary actor with a default collision box and an isolated light store.
fn fixture() -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 7,
                runtime_id: 7,
                kind: ActorKind::Entity {
                    identifier: "minecraft:zombie".into(),
                },
                position: [0.0; 3],
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
    stream
}

#[test]
fn actor_light_at_combines_channels_from_body_endpoints_across_sections() {
    let mut stream = fixture();
    let mut lower = SubChunkLight::uniform(1, 2, 1).unwrap();
    lower.set(LightChannel::Block, 4, 14, 4, 12).unwrap();
    stream
        .lighting
        .store
        .insert_resident(SubChunkKey::new(0, 0, 0, 0), lower);
    stream.lighting.store.insert_resident(
        SubChunkKey::new(0, 0, 1, 0),
        SubChunkLight::uniform(3, 11, 1).unwrap(),
    );
    let actor = stream.authority.actor(7).unwrap();
    assert_eq!(
        stream.actor_light_at(actor, [4.5, 14.5, 4.5]),
        Some((12, 11))
    );
}

#[test]
fn actor_light_at_includes_top_cell_when_body_ends_at_integer_height() {
    let mut stream = fixture();
    let mut light = SubChunkLight::uniform(0, 0, 1).unwrap();
    light.set(LightChannel::Sky, 4, 2, 4, 8).unwrap();
    stream
        .lighting
        .store
        .insert_resident(SubChunkKey::new(0, 0, 0, 0), light);
    let actor = stream.authority.actor(7).unwrap();
    assert_eq!(stream.actor_light_at(actor, [4.5, 0.2, 4.5]), Some((0, 8)));
}

#[test]
fn actor_light_at_preserves_dark_body_inside_height_and_lights_crossing_body() {
    let mut stream = fixture();
    for y in [-4, 19] {
        stream
            .lighting
            .store
            .insert_resident(SubChunkKey::new(0, 0, y, 0), SubChunkLight::dark(1));
    }
    let actor = stream.authority.actor(7).unwrap();
    for y in [-64.0, 317.0] {
        assert_eq!(stream.actor_light_at(actor, [4.5, y, 4.5]), Some((0, 0)));
    }
    for y in [-65.0, -67.0, 319.0, 321.0] {
        assert_eq!(stream.actor_light_at(actor, [4.5, y, 4.5]), Some((0, 15)));
    }
    assert_eq!(stream.actor_light_at(actor, [f32::NAN, 1.0, 4.5]), None);
    assert_eq!(stream.actor_light_at(actor, [4.5, 100.0, 4.5]), None);
}
