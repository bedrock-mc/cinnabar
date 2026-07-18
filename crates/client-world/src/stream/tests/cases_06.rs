use super::*;

#[test]
fn local_attributes_commit_without_requiring_a_local_actor_spawn() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let health = ActorAttribute {
        name: Arc::from("minecraft:health"),
        min: 0.0,
        max: 20.0,
        current: 17.5,
        default: Some(20.0),
        modifiers: Arc::from([]),
    };

    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 0,
                runtime_id: 42,
                attributes: Arc::from([health.clone()]),
                tick: 99,
            })),
        )
        .unwrap();

    assert!(stream.actor(42).is_none());
    assert_eq!(
        stream.take_committed_ui(),
        vec![CommittedUiEvent::LocalAttributes {
            sequence: 1,
            server_tick: 99,
            attributes: Arc::from([health]),
        }]
    );
}

#[test]
fn stale_dimension_local_attributes_do_not_commit_to_the_hud() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });

    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Attributes(ActorAttributesUpdateEvent {
                dimension: 1,
                runtime_id: 42,
                attributes: Arc::from([ActorAttribute {
                    name: Arc::from("minecraft:health"),
                    min: 0.0,
                    max: 20.0,
                    current: 1.0,
                    default: Some(20.0),
                    modifiers: Arc::from([]),
                }]),
                tick: 100,
            })),
        )
        .unwrap();

    assert!(stream.take_committed_ui().is_empty());
}

#[test]
fn stale_mesh_completion_cannot_replace_current_revision() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
    let decoded = DecodedLevelChunk::decode(
        -4,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .unwrap();
    stream
        .store
        .commit_level_chunk(ChunkKey::new(0, 0, 0), decoded)
        .unwrap();
    let source = stream.store.sub_chunk(key).unwrap();
    stream.resident.insert(key);
    let old_revision = stream.mark_dirty_exact(key, Instant::now());
    let current_revision = stream.mark_dirty_exact(key, Instant::now());
    stream.in_flight.insert(key, old_revision);
    let classifier = BlockClassifier::new(12_530);
    let mesh = mesh_sub_chunk(
        &classifier,
        &stream.runtime_assets,
        stream.network_id_mode,
        &Neighbourhood::empty(),
        &source,
    );
    let tint_identity = stream.biome_tint_identity();

    stream.accept_mesh_completion(MeshCompletion {
        key,
        revision: old_revision,
        source: Arc::clone(&source),
        biome_sources: biome_neighbourhood_with_center(None),
        biome: PackedBiomeRecord::fallback(),
        tint_identity,
        mesh,
        dependency_mask: MeshDependencyMask::new(false, true),
        light_halo: Default::default(),
        queue_wait: Duration::ZERO,
        duration: std::time::Duration::ZERO,
    });

    assert!(stream.revisions.is_current(key, current_revision));
    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.take_mesh_changes().is_empty());
    assert_eq!(stream.mesh_dependency_mask(key), None);
    assert_eq!(stream.pending_mesh[&key].revision, current_revision);
}

#[test]
fn mesh_dispatch_never_exceeds_the_bounded_worker_window() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
    let decoded = DecodedLevelChunk::decode(
        -4,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .unwrap();
    stream
        .store
        .commit_level_chunk(key.chunk(), decoded)
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());
    for index in 0..super::WORK_RESULT_CAPACITY {
        stream
            .in_flight
            .insert(SubChunkKey::new(7, index as i32, 0, 0), index as u64 + 1);
    }

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    assert_eq!(stream.in_flight.len(), super::WORK_RESULT_CAPACITY);
}

#[test]
fn mesh_removals_are_not_blocked_by_a_full_worker_window() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let removed = SubChunkKey::new(0, 0, -4, 0);
    stream.mark_dirty_exact(removed, Instant::now());
    for index in 0..super::WORK_RESULT_CAPACITY {
        stream
            .in_flight
            .insert(SubChunkKey::new(7, index as i32, 0, 0), index as u64 + 1);
    }

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    let changes = stream.take_mesh_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].key(), removed);
    assert!(matches!(changes[0], super::WorldMeshChange::Remove { .. }));
}

#[test]
fn final_block_removal_latency_waits_for_exact_applied_acknowledgement() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
    stream
        .store
        .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
        .unwrap();
    stream
        .store
        .update_block(key, BlockUpdate::new(0, 0, 0, 0, 12_530), 12_530)
        .unwrap();
    let dirty_since = Instant::now();
    stream.mark_dirty_exact(key, dirty_since);
    let generation = stream.revisions.dirty(key).unwrap().revision;

    stream.dispatch_mesh_jobs([0.0; 3], 0);

    assert_eq!(stream.stats().max_remesh_latency, std::time::Duration::ZERO);
    assert!(
        stream.revisions.is_current(key, generation),
        "queued removal must retain its dirty revision until render application"
    );
    let change = stream.pop_mesh_change().unwrap();
    assert_eq!(change.key(), key);
    stream.acknowledge_mesh_upload(
        key,
        generation + 1,
        dirty_since,
        dirty_since + std::time::Duration::from_millis(40),
    );
    assert_eq!(stream.stats().max_remesh_latency, std::time::Duration::ZERO);
    stream.acknowledge_mesh_upload(
        key,
        generation,
        dirty_since,
        dirty_since + std::time::Duration::from_millis(40),
    );
    assert_eq!(
        stream.stats().max_remesh_latency,
        std::time::Duration::from_millis(40)
    );
}

#[test]
fn cave_bfs_traverses_a_known_all_air_node_between_rendered_chunks() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let left = SubChunkKey::new(0, -1, 0, 0);
    let air = SubChunkKey::new(0, 0, 0, 0);
    let right = SubChunkKey::new(0, 1, 0, 0);
    stream
        .connectivity
        .insert(left, ::meshing::FaceConnectivity::all());
    stream
        .connectivity
        .insert(right, ::meshing::FaceConnectivity::all());
    stream.record_known_air(air);
    stream.mark_dirty_exact(air, Instant::now());

    stream.dispatch_mesh_jobs([0.0; 3], 0);

    assert_eq!(
        stream.connectivity(air),
        Some(::meshing::FaceConnectivity::all())
    );
    let visible = stream.cave_visible_sub_chunks(left);
    assert!(visible.contains(&air));
    assert!(visible.contains(&right));
}

#[test]
fn leaf_slab_connectivity_crosses_world_cave_graph_but_opaque_slab_stops_it() {
    let runtime_assets = Arc::new(cave_test_assets());
    let classifier = BlockClassifier::new(0);
    let leaf = cave_test_slab(1);
    let opaque = cave_test_slab(2);
    let leaf_mesh = mesh_sub_chunk(
        &classifier,
        &runtime_assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &leaf,
    );
    let opaque_mesh = mesh_sub_chunk(
        &classifier,
        &runtime_assets,
        NetworkIdMode::Sequential,
        &Neighbourhood::empty(),
        &opaque,
    );
    assert!(leaf_mesh.connectivity().is_all_connected());
    assert!(
        !opaque_mesh
            .connectivity()
            .is_connected(::meshing::Face::NegativeX, ::meshing::Face::PositiveX)
    );

    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        runtime_assets,
        [0.0; 3],
        None,
    );
    let left = SubChunkKey::new(0, -1, 0, 0);
    let middle = SubChunkKey::new(0, 0, 0, 0);
    let right = SubChunkKey::new(0, 1, 0, 0);
    let beyond_shell = SubChunkKey::new(0, 2, 0, 0);
    stream.set_connectivity(left, Some(::meshing::FaceConnectivity::all()));
    stream.set_connectivity(right, Some(::meshing::FaceConnectivity::all()));
    stream.set_connectivity(beyond_shell, Some(::meshing::FaceConnectivity::all()));
    stream.set_connectivity(middle, Some(leaf_mesh.connectivity()));

    let through_leaf = stream.cave_visible_sub_chunks(left);
    assert!(through_leaf.contains(&middle));
    assert!(through_leaf.contains(&right));
    assert!(through_leaf.contains(&beyond_shell));

    stream.set_connectivity(middle, Some(opaque_mesh.connectivity()));
    let stopped_by_opaque = stream.cave_visible_sub_chunks(left);
    assert!(stopped_by_opaque.contains(&middle));
    assert!(stopped_by_opaque.contains(&right));
    assert!(!stopped_by_opaque.contains(&beyond_shell));
}

#[test]
fn inline_zero_storage_is_a_graph_node_until_column_eviction() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 2, -3);
    let key = SubChunkKey::from_chunk(chunk, -4);
    let payload = [9, 0, (-4_i8) as u8];
    let decoded = DecodedLevelChunk::decode(-4, 1, &payload).unwrap();
    stream.apply_prepared(super::PreparedWorldEvent::InlineLevelChunk {
        event: LevelChunkEvent {
            dimension: 0,
            x: chunk.x,
            z: chunk.z,
            mode: LevelChunkMode::Inline { count: 1 },
            payload: payload.to_vec(),
        },
        decoded: Ok(decoded),
        duration: std::time::Duration::ZERO,
    });

    assert!(stream.store.sub_chunk(key).is_none());
    assert!(stream.resident.contains(&key));
    assert!(stream.known_air.contains(&key));
    assert!(stream.block_generations.contains_key(&key));
    assert!(stream.pending_light.contains_key(&key));
    assert_eq!(
        stream.light_store.kind(key),
        world::LightSubChunkKind::KnownAir
    );
    assert_eq!(
        stream.connectivity(key),
        Some(::meshing::FaceConnectivity::all())
    );

    stream.evict_column(chunk);
    assert!(!stream.resident.contains(&key));
    assert!(!stream.known_air.contains(&key));
    assert_eq!(stream.connectivity(key), None);
}

#[test]
fn explicit_all_air_result_is_counted_as_a_resident_graph_node() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 1,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(1, -8, 3, 12);
    stream
        .requested_sub_chunks
        .insert(key.chunk(), BTreeMap::from([(key.y, Default::default())]));
    stream.apply_prepared(super::PreparedWorldEvent::SubChunks {
        dimension: key.dimension,
        entries: vec![super::PreparedSubChunk {
            position: [key.x, key.y, key.z],
            result: super::PreparedSubChunkResult::AllAir,
        }],
        duration: std::time::Duration::ZERO,
    });

    assert!(stream.resident.contains(&key));
    assert!(stream.known_air.contains(&key));
    assert_eq!(
        stream.connectivity(key),
        Some(::meshing::FaceConnectivity::all())
    );
    assert_eq!(stream.stats().resident_sub_chunks, 1);
}

#[test]
fn connectivity_generation_changes_only_when_the_graph_changes() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 2, -1, 4);

    assert_eq!(stream.connectivity_generation(), 0);
    stream.record_known_air(key);
    let inserted = stream.connectivity_generation();
    assert_ne!(inserted, 0);

    stream.record_known_air(key);
    assert_eq!(stream.connectivity_generation(), inserted);

    stream.evict_column(key.chunk());
    assert_ne!(stream.connectivity_generation(), inserted);
}

#[test]
fn surface_spawn_waits_for_level_chunk_commit_and_treats_omitted_top_as_air() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 2, -3);
    let block_x = chunk.x * 16 + 5;
    let block_z = chunk.z * 16 + 7;
    assert_eq!(stream.surface_eye_position(block_x, block_z), None);

    let payload = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../world/fixtures/uniform_non_air.bin"
    ));
    let decoded = DecodedLevelChunk::decode(-4, 1, payload).unwrap();
    stream.apply_prepared(super::PreparedWorldEvent::InlineLevelChunk {
        event: LevelChunkEvent {
            dimension: 0,
            x: chunk.x,
            z: chunk.z,
            mode: LevelChunkMode::Inline { count: 1 },
            payload: payload.to_vec(),
        },
        decoded: Ok(decoded),
        duration: std::time::Duration::ZERO,
    });

    assert_eq!(
        stream.surface_eye_position(block_x, block_z),
        Some([block_x as f32 + 0.5, -46.38, block_z as f32 + 0.5])
    );
}

#[test]
fn actor_ingestion_is_fifo_visible_without_dirtying_chunk_meshes() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let before = stream.stats();
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 7,
                runtime_id: 8,
                kind: ActorKind::Entity {
                    identifier: "minecraft:bee".into(),
                },
                game_mode: None,
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

    assert_eq!(stream.actor_count(), 1);
    assert_eq!(stream.actor(8).unwrap().position, [1.0, 2.0, 3.0]);
    assert!(stream.take_mesh_changes().is_empty());
    let after = stream.stats();
    assert_eq!(after.pending_mesh_jobs, before.pending_mesh_jobs);
    assert_eq!(after.in_flight_mesh_jobs, before.in_flight_mesh_jobs);
}

#[test]
fn player_spawn_move_player_and_absolute_move_share_feet_space() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 7,
                runtime_id: 8,
                kind: ActorKind::Player {
                    uuid: [8; 16],
                    username: "Alex".into(),
                },
                game_mode: Some(protocol::ActorGameMode::Survival),
                position: [1.0, 64.0, 2.0],
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
    stream
        .submit(
            2,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 8,
                position: [1.0, 64.0 + PLAYER_NETWORK_OFFSET, 2.0],
                on_ground: true,
                ..Default::default()
            }),
        )
        .unwrap();
    stream.advance_actor_interpolation_ticks(3);
    assert_eq!(stream.actor(8).unwrap().position, [1.0, 64.0, 2.0]);

    stream
        .submit(
            3,
            WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 8,
                position: [Some(1.0), Some(64.0 + PLAYER_NETWORK_OFFSET), Some(2.0)],
                position_origin: ActorPositionOrigin::NetworkOffset,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: true,
                snap: true,
                player_mode: None,
                source_tick: None,
            })),
        )
        .unwrap();
    let actor = stream.actor(8).unwrap();
    assert_eq!(actor.previous_pose.position, [1.0, 64.0, 2.0]);
    assert_eq!(actor.position, [1.0, 64.0, 2.0]);
    assert_eq!(actor.received_pose.position, [1.0, 64.0, 2.0]);
}
