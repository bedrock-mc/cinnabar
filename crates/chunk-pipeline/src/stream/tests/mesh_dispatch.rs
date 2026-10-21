use super::*;

#[test]
fn starved_publication_tokens_still_dispatch_when_nothing_is_in_flight() {
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
    stream.resident.insert(key);
    stream.mark_light_changed_sources([key]);
    light_scheduler::settle_light(&mut stream, [0.0; 3]);
    stream.mark_dirty_exact(key, Instant::now());

    // Exhaust the frame publication window entirely: without the starved
    // dispatch floor this poll could never start meshing again.
    let config = render_api::PublicationServiceConfig::DEFAULT;
    let allowance = render_api::PublicationAllowance::new(config);
    allowance.begin_frame(
        1,
        config.minimum_items_per_second as usize,
        config.maximum_burst_bytes,
        config.maximum_zero_byte_operations_per_frame,
        0,
    );
    stream.set_publication_allowance(allowance);

    assert_eq!(stream.poll([0.0; 3], 32).mesh_jobs_dispatched, 1);
}

#[test]
fn starved_mesh_dispatch_floor_admits_exactly_the_floor_through_poll() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    // Six dispatchable resident meshes exceed the floor, so the exact-count
    // assertion below discriminates the floor's value: the single-job witness
    // above passes for any nonzero or unbounded floor.
    let keys = (0..6)
        .map(|x| SubChunkKey::new(0, x, -4, 0))
        .collect::<Vec<_>>();
    for key in &keys {
        stream
            .authority
            .update_block(
                *key,
                BlockUpdate::new(0, 0, 0, 0, 99),
                protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            )
            .unwrap();
        stream.resident.insert(*key);
    }
    stream.mark_light_changed_sources(keys.iter().copied());
    light_scheduler::settle_light(&mut stream, [0.0; 3]);
    for key in &keys {
        stream.mark_dirty_exact(*key, Instant::now());
    }
    assert_eq!(stream.mesh_jobs.pending.len(), 6);
    assert!(stream.mesh_jobs.in_flight.is_empty());

    // Exhaust the frame publication window exactly like the single-job
    // witness above: the starved floor is the only remaining admission
    // route through the real poll path.
    let config = render_api::PublicationServiceConfig::DEFAULT;
    let allowance = render_api::PublicationAllowance::new(config);
    allowance.begin_frame(
        1,
        config.minimum_items_per_second as usize,
        config.maximum_burst_bytes,
        config.maximum_zero_byte_operations_per_frame,
        0,
    );
    stream.set_publication_allowance(allowance);

    let report = stream.poll([0.0; 3], 32);
    assert_eq!(report.mesh_jobs_dispatched, 4);
    assert_eq!(
        stream.mesh_jobs.in_flight.len(),
        4,
        "exactly the floored budget must enter the worker window"
    );
}

#[test]
fn starved_mesh_dispatch_floor_never_exceeds_the_callers_budget() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let keys = (0..6)
        .map(|x| SubChunkKey::new(0, x, -4, 0))
        .collect::<Vec<_>>();
    for key in &keys {
        stream
            .authority
            .update_block(
                *key,
                BlockUpdate::new(0, 0, 0, 0, 99),
                protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            )
            .unwrap();
        stream.resident.insert(*key);
    }
    stream.mark_light_changed_sources(keys.iter().copied());
    light_scheduler::settle_light(&mut stream, [0.0; 3]);
    for key in &keys {
        stream.mark_dirty_exact(*key, Instant::now());
    }
    let config = render_api::PublicationServiceConfig::DEFAULT;
    let allowance = render_api::PublicationAllowance::new(config);
    allowance.begin_frame(
        1,
        config.minimum_items_per_second as usize,
        config.maximum_burst_bytes,
        config.maximum_zero_byte_operations_per_frame,
        0,
    );
    stream.set_publication_allowance(allowance);

    assert_eq!(stream.poll([0.0; 3], 2).mesh_jobs_dispatched, 2);
}

#[test]
fn starved_mesh_dispatch_floor_never_invents_work_with_an_empty_pending_queue() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    // Same exhausted publication window as the floor witness, but with no
    // pending mesh work anywhere: starvation must not fabricate dispatches.
    let config = render_api::PublicationServiceConfig::DEFAULT;
    let allowance = render_api::PublicationAllowance::new(config);
    allowance.begin_frame(
        1,
        config.minimum_items_per_second as usize,
        config.maximum_burst_bytes,
        config.maximum_zero_byte_operations_per_frame,
        0,
    );
    stream.set_publication_allowance(allowance);

    let report = stream.poll([0.0; 3], 32);
    assert_eq!(report.mesh_jobs_dispatched, 0);
    assert!(stream.mesh_jobs.in_flight.is_empty());
    assert!(stream.take_mesh_changes().is_empty());
}

#[test]
fn stale_mesh_completion_cannot_replace_current_revision() {
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
    let decoded = DecodedLevelChunk::decode(
        -4,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
        &RAW_IDS,
    );
    stream
        .authority
        .commit_level_chunk(ChunkKey::new(0, 0, 0), decoded)
        .unwrap();
    let source = stream.authority.terrain().sub_chunk(key).unwrap();
    stream.resident.insert(key);
    let old_revision = stream.mark_dirty_exact(key, Instant::now());
    let current_revision = stream.mark_dirty_exact(key, Instant::now());
    stream.mesh_jobs.in_flight.insert(key, old_revision);
    let classifier = BlockClassifier::new(protocol_ids::SEQUENTIAL_AIR_NETWORK_ID);
    let mesh = mesh_sub_chunk(
        &classifier,
        stream.runtime_assets(),
        stream.network_id_mode(),
        &Neighbourhood::empty(),
        &source,
    );
    let tint_identity = stream.biome_tint_identity();

    stream.accept_mesh_completion(MeshCompletion {
        output_permit: None,
        _job_permit: None,
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
        dispatch_wait: Duration::ZERO,
        duration: std::time::Duration::ZERO,
        urgent: false,
    });

    assert!(stream.revisions.is_current(key, current_revision));
    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.take_mesh_changes().is_empty());
    assert_eq!(stream.mesh_dependency_mask(key), None);
    assert_eq!(stream.mesh_jobs.pending[&key].revision, current_revision);
}

#[test]
fn mesh_dispatch_never_exceeds_the_bounded_worker_window() {
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
    let decoded = DecodedLevelChunk::decode(
        -4,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
        &RAW_IDS,
    );
    stream
        .authority
        .commit_level_chunk(key.chunk(), decoded)
        .unwrap();
    stream.resident.insert(key);
    stream.mark_changed(key, Instant::now());
    for index in 0..super::WORK_RESULT_CAPACITY {
        stream
            .mesh_jobs
            .in_flight
            .insert(SubChunkKey::new(7, index as i32, 0, 0), index as u64 + 1);
    }

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 1), 0);
    assert_eq!(
        stream.mesh_jobs.in_flight.len(),
        super::WORK_RESULT_CAPACITY
    );
}

#[test]
fn actor_ingestion_is_fifo_visible_without_dirtying_chunk_meshes() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
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
                links: Arc::from([]),
            })),
        )
        .unwrap();

    assert_eq!(stream.authority().actor_count(), 1);
    assert_eq!(
        stream.authority().actor(8).unwrap().position,
        [1.0, 2.0, 3.0]
    );
    assert!(stream.take_mesh_changes().is_empty());
    let after = stream.stats();
    assert_eq!(after.pending_mesh_jobs, before.pending_mesh_jobs);
    assert_eq!(after.in_flight_mesh_jobs, before.in_flight_mesh_jobs);
}
