use super::*;

#[test]
fn pickup_commits_one_sound_even_when_the_item_is_removed_in_the_same_batch() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    let network_id = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:apple")
        .unwrap()
        .network_id;
    let spawn = ActorEvent::Spawn(protocol::ActorSpawnEvent {
        dimension: 0,
        unique_id: 7,
        runtime_id: 7,
        kind: protocol::ActorKind::Entity {
            identifier: "minecraft:item".into(),
        },
        position: [1.0, 2.0, 3.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: protocol::NetworkItemStack {
            network_id,
            count: 1,
            ..Default::default()
        },
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    });
    let pickup = ActorEvent::TakeItem(protocol::ActorTakeItemEvent {
        item_runtime_id: 7,
        collector_runtime_id: 1,
    });
    for (index, event) in [
        spawn,
        pickup.clone(),
        pickup,
        ActorEvent::Remove(protocol::ActorRemoveEvent {
            dimension: 0,
            unique_id: 7,
        }),
    ]
    .into_iter()
    .enumerate()
    {
        authority
            .apply_ordered_event(WorldEvent::Actor(event), Some(index as u64 + 1))
            .unwrap();
    }
    assert!(authority.actor(7).is_none());
    let sounds = authority.take_committed_audio();
    assert_eq!(sounds.len(), 1);
    assert_eq!(sounds[0].sequence, 2);
    let AudioEvent::Level(sound) = &sounds[0].event else {
        panic!("pickup uses pack routing")
    };
    assert_eq!(sound.sound_event.as_ref(), "pop");
    assert_eq!(
        sound.position,
        [1.0, 2.0 + protocol::ITEM_ACTOR_NETWORK_OFFSET, 3.0]
    );
    assert!(authority.take_committed_audio().is_empty());
}
