use super::*;

#[test]
fn undrained_ui_commits_apply_bounded_backpressure_without_panicking() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    for sequence in 1..=MAX_ADMITTED_WORLD_EVENTS as u64 {
        stream
            .submit(
                sequence,
                WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 20 })),
            )
            .unwrap();
    }

    assert_eq!(stream.remaining_admission_capacity(), 0);
    assert!(matches!(
        stream.submit(
            MAX_ADMITTED_WORLD_EVENTS as u64 + 1,
            WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 19 })),
        ),
        Err(WorldStreamError::AdmissionFull { .. })
    ));
    assert_eq!(stream.take_committed_ui().len(), MAX_ADMITTED_WORLD_EVENTS);
}

#[test]
fn same_location_reset_evicts_stale_state_and_consumes_the_next_publisher_epoch() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [8.5, 70.0, 8.5],
        world_spawn_position: [8, 70, 8],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [8, 70, 8],
                radius_blocks: 128,
            }),
        )
        .unwrap();
    stream
        .submit(
            3,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 1 }, 1),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let player_column = ChunkKey::new(0, 0, 0);
    let known_air = SubChunkKey::new(0, 0, 1, 0);
    stream.record_known_air(known_air);
    let old_mesh_generation = stream.revisions.mark_dirty(known_air, Instant::now());
    stream.revisions.entries.remove(&known_air);
    stream
        .applied_mesh_generations
        .insert(known_air, old_mesh_generation);
    stream.next_block_generation = 41;
    stream.light_revisions.next_revision = 73;
    let connectivity_generation = stream.connectivity_generation;
    assert!(!stream.tracked_columns().is_empty());
    assert!(!stream.required_columns.is_empty());
    let epoch_before = stream.publisher_epoch;

    stream
        .submit(
            4,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 7,
                runtime_id: 8,
                kind: ActorKind::Entity {
                    identifier: "minecraft:bee".into(),
                },
                position: [1.0, 2.0, 3.0],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
            })),
        )
        .unwrap();
    assert!(stream.actor(8).is_some());
    stream.submit_same_location_reset(5).unwrap();

    let armed = stream.phase2_publication_snapshot(player_column);
    assert_eq!(armed.publisher_center, Some([8, 70, 8]));
    assert!(armed.local_reset_armed);
    assert_eq!(armed.local_resets_armed, 1);
    assert_eq!(armed.local_resets_consumed, 0);
    assert_eq!(armed.required_columns, 0);
    assert!(stream.committed_view_cohort().is_none());
    assert!(stream.tracked_columns().is_empty());
    assert_eq!(stream.pending_request_work_count(), 0);
    assert_eq!(stream.outstanding_sub_chunk_count(), 0);
    assert!(stream.actor(8).is_none());
    assert_eq!(stream.next_block_generation, 41);
    assert_eq!(stream.light_revisions.next_revision, 73);
    assert!(stream.connectivity_generation > connectivity_generation);
    assert!(
        stream
            .revisions
            .dirty(known_air)
            .is_some_and(|dirty| dirty.revision > old_mesh_generation)
    );

    stream
        .submit(
            6,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [8, 70, 8],
                radius_blocks: 128,
            }),
        )
        .unwrap();

    let consumed = stream.phase2_publication_snapshot(player_column);
    assert!(!consumed.local_reset_armed);
    assert_eq!(consumed.local_resets_armed, 1);
    assert_eq!(consumed.local_resets_consumed, 1);
    assert_eq!(consumed.publisher_epoch, epoch_before + 1);
}

#[test]
fn same_location_reset_waits_for_older_decode_then_replacement_repopulates() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [8.5, 70.0, 8.5],
        world_spawn_position: [8, 70, 8],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = ChunkKey::new(0, 0, 0);
    let stale_air = SubChunkKey::new(0, 0, 1, 0);
    stream.record_known_air(stale_air);

    stream.submit(1, inline_air_event(0)).unwrap();
    stream.submit_same_location_reset(2).unwrap();
    stream.submit(3, inline_air_event(0)).unwrap();

    assert!(stream.has_pending_same_location_reset());
    assert!(stream.tracked_columns().contains(&key));
    assert_eq!(stream.pending_decode.len(), 2);

    complete_pending_decode_jobs(&mut stream);

    assert!(!stream.has_pending_same_location_reset());
    assert!(stream.loaded_columns.contains(&key));
    assert!(stream.tracked_columns().contains(&key));
    assert!(!stream.known_air.contains(&stale_air));
    assert!(stream.revisions.next_revision > 0);
}
