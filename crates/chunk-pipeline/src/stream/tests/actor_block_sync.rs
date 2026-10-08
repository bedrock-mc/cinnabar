use super::*;
use protocol::{ActorBlockSyncMessage, ActorMetadata, ActorMetadataValue, SyncedBlockUpdateEvent};

fn fixture() -> (WorldStream, SubChunkKey) {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
    stream.record_known_air(key);
    stream.mark_live_mutation_changed(key, Instant::now(), false);
    acknowledge_current(&mut stream, key);
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: -2,
                runtime_id: 7,
                kind: ActorKind::Entity {
                    identifier: "minecraft:falling_block".into(),
                },
                position: [0.5, -63.5, 0.5],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([ActorMetadata {
                    key: 2,
                    value: ActorMetadataValue::Int(55),
                }]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    synced_update(&mut stream, 2, 0, protocol::SEQUENTIAL_AIR_NETWORK_ID, 1);
    (stream, key)
}

fn synced_update(
    stream: &mut WorldStream,
    sequence: u64,
    layer: usize,
    network_id: u32,
    message: u64,
) {
    synced_update_with_flags(stream, sequence, layer, network_id, message, 0x13);
}

fn synced_update_with_flags(
    stream: &mut WorldStream,
    sequence: u64,
    layer: usize,
    network_id: u32,
    message: u64,
    flags: u32,
) {
    stream
        .submit(
            sequence,
            WorldEvent::SyncedBlockUpdates(vec![SyncedBlockUpdateEvent {
                update: BlockUpdateEvent {
                    dimension: 0,
                    position: [0, -64, 0],
                    layer,
                    network_id,
                },
                flags,
                sync: ActorBlockSyncMessage {
                    actor_unique_id: -2,
                    message,
                },
            }]),
        )
        .unwrap();
    complete_pending_decode_jobs(stream);
}

#[test]
fn falling_sync_respects_render_notification_flags_without_delaying_collision_updates() {
    for flags in [0, 1, 6, 3] {
        let (mut stream, key) = fixture();
        let network_id = if flags == 3 {
            protocol::SEQUENTIAL_AIR_NETWORK_ID
        } else {
            99
        };
        synced_update_with_flags(&mut stream, 3, 0, network_id, 2, flags);
        if flags != 3 {
            assert_eq!(
                stream
                    .collision_store()
                    .sub_chunk(key)
                    .unwrap()
                    .runtime_id(0, 0, 0, 0),
                Some(network_id),
                "notification flags cannot defer accepted world writes"
            );
            acknowledge_current(&mut stream, key);
        }
        assert_eq!(
            stream.authority().block_entities(1.0).len(),
            1,
            "flags {flags:#x} do not notify the renderer"
        );
    }
}

fn acknowledge_current(stream: &mut WorldStream, key: SubChunkKey) {
    let dirty = stream.revisions.dirty(key).unwrap();
    stream.mesh_jobs.pending.remove(&key);
    stream.acknowledge_mesh_upload(key, dirty.revision, dirty.since, Instant::now());
}

#[test]
fn falling_landing_updates_collision_before_mesh_and_hides_only_after_current_upload() {
    let (mut stream, key) = fixture();
    let old_generation = stream.applied_mesh_generations[&key];
    synced_update(&mut stream, 3, 0, 99, 2);
    assert_eq!(
        stream
            .collision_store()
            .sub_chunk(key)
            .unwrap()
            .runtime_id(0, 0, 0, 0),
        Some(99)
    );
    assert_eq!(
        stream.authority().block_entities(1.0).len(),
        1,
        "actor bridges the unpublished terrain mesh"
    );
    let dirty = stream.revisions.dirty(key).unwrap();
    stream.acknowledge_mesh_upload(key, old_generation, dirty.since, Instant::now());
    assert_eq!(
        stream.authority().block_entities(1.0).len(),
        1,
        "stale upload cannot release a landing fence"
    );
    acknowledge_current(&mut stream, key);
    assert!(
        stream.authority().block_entities(1.0).is_empty(),
        "published landing geometry replaces the actor visual"
    );
    assert_eq!(
        stream.authority().actor(7).unwrap().unique_id,
        -2,
        "the actor remains authoritative"
    );
}

#[test]
fn falling_landing_noop_uses_the_clean_published_terrain() {
    let (mut stream, key) = fixture();
    synced_update(&mut stream, 3, 0, 99, 1);
    acknowledge_current(&mut stream, key);
    assert_eq!(stream.authority().block_entities(1.0).len(), 1);
    synced_update(&mut stream, 4, 0, 99, 2);
    assert!(
        stream.revisions.dirty(key).is_none(),
        "unchanged published terrain needs no rebuild"
    );
    assert!(stream.authority().block_entities(1.0).is_empty());
}

#[test]
fn falling_landing_releases_on_a_newer_coalesced_mesh_upload() {
    let (mut stream, key) = fixture();
    synced_update(&mut stream, 3, 0, 99, 2);
    let replaced = stream.revisions.dirty(key).unwrap();
    stream.mark_dirty_exact(key, Instant::now());
    stream.acknowledge_mesh_upload(key, replaced.revision, replaced.since, Instant::now());
    assert_eq!(stream.authority().block_entities(1.0).len(), 1);
    acknowledge_current(&mut stream, key);
    assert!(
        stream.authority().block_entities(1.0).is_empty(),
        "new mesh includes earlier block transitions"
    );
}

#[test]
fn falling_sync_messages_release_fifo_within_a_coalesced_generation() {
    let (mut stream, key) = fixture();
    synced_update(&mut stream, 3, 0, 99, 2);
    synced_update(&mut stream, 4, 0, 99, 1);
    acknowledge_current(&mut stream, key);
    assert_eq!(stream.authority().block_entities(1.0).len(), 1);
    stream
        .authority
        .apply_actor_block_sync(ActorBlockSyncMessage {
            actor_unique_id: -2,
            message: 2,
        });
    stream.acknowledge_actor_block_syncs(key, u64::MAX);
    assert!(
        stream.authority().block_entities(1.0).is_empty(),
        "released messages cannot replay"
    );
}

#[test]
fn falling_sync_for_extra_storage_does_not_change_actor_visibility() {
    let (mut stream, key) = fixture();
    synced_update(&mut stream, 3, 1, 99, 2);
    assert_eq!(
        stream
            .collision_store()
            .sub_chunk(key)
            .unwrap()
            .runtime_id(1, 0, 0, 0),
        Some(99)
    );
    acknowledge_current(&mut stream, key);
    assert_eq!(stream.authority().block_entities(1.0).len(), 1);
}

#[test]
fn falling_sync_for_unpublished_noop_air_waits_for_empty_mesh_ack() {
    let (mut stream, key) = fixture();
    stream.applied_mesh_generations.remove(&key);
    synced_update(&mut stream, 3, 0, protocol::SEQUENTIAL_AIR_NETWORK_ID, 2);
    assert_eq!(stream.authority().block_entities(1.0).len(), 1);
    acknowledge_current(&mut stream, key);
    assert!(stream.authority().block_entities(1.0).is_empty());
    synced_update(&mut stream, 4, 0, protocol::SEQUENTIAL_AIR_NETWORK_ID, 1);
    assert!(
        stream.revisions.dirty(key).is_none(),
        "empty acknowledged terrain stays clean"
    );
    assert_eq!(stream.authority().block_entities(1.0).len(), 1);
}

#[test]
fn falling_sync_is_pruned_when_its_terrain_is_evicted() {
    let (mut stream, key) = fixture();
    synced_update(&mut stream, 3, 0, 99, 2);
    stream
        .actor_block_syncs
        .remove_columns(&BTreeSet::from([key.chunk()]));
    acknowledge_current(&mut stream, key);
    assert_eq!(stream.authority().block_entities(1.0).len(), 1);
}

#[test]
fn falling_render_fences_preserve_handoff_order_across_source_and_landing_sections() {
    let (mut stream, landing) = fixture();
    let source = SubChunkKey::new(0, 0, -3, 0);
    stream.record_known_air(source);
    stream.mark_live_mutation_changed(source, Instant::now(), false);
    stream.queue_actor_block_syncs(vec![SyncedBlockUpdateEvent {
        update: BlockUpdateEvent {
            dimension: 0,
            position: [0, -48, 0],
            layer: 0,
            network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        },
        flags: 0x13,
        sync: ActorBlockSyncMessage {
            actor_unique_id: -2,
            message: 1,
        },
    }]);
    synced_update(&mut stream, 3, 0, 99, 2);
    let fences = stream.actor_block_sync_fences();
    assert_eq!(fences.len(), 2);
    assert_eq!(
        fences
            .iter()
            .map(|fence| fence.sync.message)
            .collect::<Vec<_>>(),
        [1, 2],
        "the last completed handoff must hide the actor, independent of section ordering"
    );
    assert_eq!([fences[0].key, fences[1].key], [source, landing]);
    acknowledge_current(&mut stream, source);
    assert_eq!(stream.actor_block_sync_fences().len(), 1);
    acknowledge_current(&mut stream, landing);
    assert!(stream.actor_block_sync_fences().is_empty());
}
