use super::*;

#[test]
fn cave_bfs_traverses_a_known_all_air_node_between_rendered_chunks() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
            local_player_unique_id: 1,
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
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let chunk = ChunkKey::new(0, 2, -3);
    let key = SubChunkKey::from_chunk(chunk, -4);
    let payload = [9, 0, (-4_i8) as u8];
    let decoded = DecodedLevelChunk::decode(-4, 1, &payload, &RAW_IDS);
    stream.apply_prepared(super::PreparedWorldEvent::InlineLevelChunk {
        event: LevelChunkEvent {
            dimension: 0,
            x: chunk.x,
            z: chunk.z,
            mode: LevelChunkMode::Inline { count: 1 },
            payload: payload.to_vec(),
        },
        decoded,
        light_neutral: Vec::new(),
        duration: std::time::Duration::ZERO,
    });

    assert!(stream.authority.terrain().sub_chunk(key).is_none());
    assert!(stream.resident.contains(&key));
    assert!(stream.known_air.contains(&key));
    assert!(stream.lighting.block_generations.contains_key(&key));
    assert!(stream.lighting.jobs.pending.contains_key(&key));
    assert_eq!(
        stream.lighting.store.kind(key),
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
        local_player_unique_id: 1,
        dimension: 1,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(1, -8, 3, 12);
    stream
        .requests
        .requested
        .insert(key.chunk(), BTreeMap::from([(key.y, Default::default())]));
    stream.apply_prepared(super::PreparedWorldEvent::SubChunks {
        dimension: key.dimension,
        entries: vec![super::PreparedSubChunk {
            diagnostics: None,
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
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
