use super::*;

#[test]
fn synchronized_audio_ordered_ingress_waits_for_fixed_tick_and_preserves_admission_credit() {
    let mut authority = WorldAuthority::new(
        WorldBootstrap {
            local_player_unique_id: 1,
            local_player_runtime_id: 1,
            dimension: 0,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        None,
        [0.0; 3],
        None,
    );
    authority
        .apply_ordered_event(
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 17,
                runtime_id: 7,
                kind: ActorKind::Entity {
                    identifier: "minecraft:ender_dragon".into(),
                },
                position: [1.0, 64.0, 3.0],
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
            Some(10),
        )
        .unwrap();
    let sound = protocol::AudioEvent::Level(protocol::LevelAudioEvent {
        sound_event: "death".into(),
        position: [1.0, 65.0, 3.0],
        data: -1,
        actor_identifier: "minecraft:ender_dragon".into(),
        is_baby: false,
        is_global: false,
        actor_unique_id: 17,
        fire_at_position: Some([1.0, 64.0, 3.0]),
    });
    authority
        .apply_ordered_event(WorldEvent::Audio(sound.clone()), Some(11))
        .unwrap();
    assert!(
        authority.take_committed_audio().is_empty(),
        "the optional fire position selects actor tick synchronization"
    );
    assert_eq!(authority.retained_commit_count(), 1);
    authority.advance_actor_interpolation_frame(0);
    assert!(authority.take_committed_audio().is_empty());
    authority.advance_actor_interpolation_frame(1);
    assert_eq!(
        authority.retained_commit_count(),
        1,
        "publication transfers existing admission credit"
    );
    let events = authority.take_committed_audio();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].sequence, 11);
    assert_eq!(events[0].event, sound);
    assert_eq!(events[0].dimension_epoch, 0);
    let owner = events[0].actor_synchronization.unwrap();
    assert_eq!(owner.runtime_id, 7);
    assert_eq!(owner.dimension, 0);
    assert_eq!(authority.retained_commit_count(), 0);
    authority.advance_actor_interpolation_frame(5);
    assert!(authority.take_committed_audio().is_empty());
}
