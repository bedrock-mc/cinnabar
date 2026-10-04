use super::*;

#[test]
fn urgent_mesh_completion_retry_stays_at_the_front() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, 0, 0);
    let revision = stream.mark_dirty_exact(key, Instant::now());
    stream.mesh_jobs.pending.remove(&key);
    stream.mesh_jobs.scan.clear();

    stream.requeue_current_mesh_completion(key, revision, true);

    assert!(stream.mesh_jobs.pending[&key].urgent);
    assert_eq!(stream.mesh_jobs.scan.front(), Some(&(key, revision)));
}

#[test]
fn remote_projectile_motion_replaces_the_retained_velocity() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let spawn = ActorSpawnEvent {
        dimension: 0,
        unique_id: 77,
        runtime_id: 77,
        kind: ActorKind::Entity {
            identifier: "minecraft:ender_pearl".into(),
        },
        position: [0.0; 3],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: protocol::NetworkItemStack::empty(),
        metadata: Arc::from([]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    };
    stream
        .submit(1, WorldEvent::Actor(ActorEvent::Spawn(spawn)))
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::ActorMotion(ActorMotionEvent {
                actor_runtime_id: 77,
                motion: [0.5, 0.2, -0.75],
                tick: 7,
            }),
        )
        .unwrap();
    assert_eq!(
        stream.authority().actor(77).unwrap().velocity,
        [0.5, 0.2, -0.75]
    );
    assert!(stream.take_committed_controls().is_empty());
}
