use super::*;

#[test]
fn mesh_removals_are_not_blocked_by_a_full_worker_window() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let removed = SubChunkKey::new(0, 0, -4, 0);
    stream.mark_dirty_exact(removed, Instant::now());
    for index in 0..super::WORK_RESULT_CAPACITY {
        stream
            .mesh_jobs
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
fn removal_waits_for_source_authority_and_carries_its_zero_byte_permit() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let allowance = super::PublicationAllowance::new(super::PublicationServiceConfig::DEFAULT);
    allowance.begin_frame(
        1,
        0,
        0,
        0,
        super::PublicationServiceConfig::DEFAULT.maximum_frame_items,
    );
    stream.set_publication_allowance(allowance.clone());
    let removed = SubChunkKey::new(0, 0, -4, 0);
    stream.mark_dirty_exact(removed, Instant::now());

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    assert!(stream.take_mesh_changes().is_empty());

    allowance.begin_frame(
        2,
        1,
        0,
        1,
        super::PublicationServiceConfig::DEFAULT.maximum_frame_items,
    );
    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    let mut changes = stream.take_mesh_changes();
    assert_eq!(changes.len(), 1);
    let super::WorldMeshChange::Remove {
        permit: Some(permit),
        ..
    } = changes.pop().unwrap()
    else {
        panic!("expected one permitted removal")
    };
    assert_eq!(
        permit.stage(),
        Some(super::PublicationPermitStage::MeshReady)
    );
    assert!(permit.retire());
}

#[test]
fn removal_heavy_mesh_work_prioritizes_real_meshes_and_respects_poll_budget() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let real = SubChunkKey::new(0, 100, -4, 0);
    let decoded = DecodedLevelChunk::decode(
        real.y,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
        &RAW_IDS,
    );
    stream
        .authority
        .commit_level_chunk(real.chunk(), decoded)
        .unwrap();
    stream.resident.insert(real);
    let block_generation = 7;
    let light_revision = 11;
    stream
        .lighting
        .block_generations
        .insert(real, block_generation);
    stream
        .lighting
        .store
        .insert_resident(real, SubChunkLight::dark(light_revision));
    stream.lighting.ownership.insert(
        real,
        LightOwnership {
            block_generation,
            light_revision,
        },
    );
    stream.lighting.direct_sky.insert(
        real,
        StoredDirectSky {
            light_revision,
            mask: Arc::new(DirectSkyMask::Uniform(false)),
        },
    );
    let real_revision = stream.mark_dirty_exact(real, Instant::now());

    let removals = (-4..=4)
        .flat_map(|x| (-4..=4).flat_map(move |y| (-4..=4).map(move |z| (x, y, z))))
        .take(super::MAX_PENDING_MESH_CHANGES)
        .map(|(x, y, z)| SubChunkKey::new(0, x, y, z))
        .collect::<Vec<_>>();
    for key in &removals {
        stream.mark_dirty_exact(*key, Instant::now());
    }

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 2), 1);
    assert_eq!(
        stream.mesh_jobs.in_flight.get(&real).copied(),
        Some(real_revision)
    );
    assert!(stream.pending_mesh_change_count() <= 2);
    assert_eq!(
        stream
            .mesh_jobs
            .pending
            .keys()
            .filter(|key| removals.contains(key))
            .count(),
        removals
            .len()
            .saturating_sub(stream.pending_mesh_change_count())
    );
}

#[test]
fn final_block_removal_latency_waits_for_exact_applied_acknowledgement() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 0, -4, 0);
    stream
        .authority
        .update_block(
            key,
            BlockUpdate::new(0, 0, 0, 0, 99),
            protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        )
        .unwrap();
    stream
        .authority
        .update_block(
            key,
            BlockUpdate::new(0, 0, 0, 0, protocol_ids::SEQUENTIAL_AIR_NETWORK_ID),
            protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        )
        .unwrap();
    let dirty_since = Instant::now();
    stream.mark_dirty_exact(key, dirty_since);
    let generation = stream.revisions.dirty(key).unwrap().revision;

    stream.dispatch_mesh_jobs([0.0; 3], 1);

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

/// Removal acks for evicted sub-chunks must not accumulate applied generations.
#[test]
fn evicted_removal_acks_leave_no_applied_generation_behind() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let now = Instant::now();
    let resident = SubChunkKey::new(0, 0, 0, 0);
    let evicted = SubChunkKey::new(0, 40, 0, 0);
    stream.resident.insert(resident);
    for key in [resident, evicted] {
        let generation = stream.mark_dirty_exact(key, now);
        stream.acknowledge_mesh_upload(key, generation, now, now);
    }
    assert!(stream.applied_mesh_generations.contains_key(&resident));
    assert!(!stream.applied_mesh_generations.contains_key(&evicted));
    assert_eq!(stream.applied_mesh_generations.len(), 1);
}
