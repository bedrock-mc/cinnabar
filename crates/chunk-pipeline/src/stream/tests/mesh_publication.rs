use super::*;

#[test]
fn render_backpressure_retry_preserves_change_order_for_eventual_delivery() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let first = SubChunkKey::new(0, 1, 2, 3);
    let second = SubChunkKey::new(0, 4, 5, 6);
    stream.mesh_changes.push(super::WorldMeshChange::Remove {
        key: first,
        generation: 1,
        dirty_since: Instant::now(),
        urgent: false,
        permit: None,
    });
    stream.mesh_changes.push(super::WorldMeshChange::Remove {
        key: second,
        generation: 2,
        dirty_since: Instant::now(),
        urgent: false,
        permit: None,
    });
    stream.stats.publication_stages.mesh_changes_queued = 2;

    let blocked = stream.pop_mesh_change().unwrap();
    stream.retry_mesh_change_front(blocked).unwrap();

    let stages = stream.publication_snapshot(ChunkKey::new(0, 0, 0)).stages;
    assert_eq!(stages.mesh_changes_queued, 3);
    assert_eq!(stages.mesh_changes_dequeued, 1);
    assert_eq!(stages.mesh_changes_pending, 2);
    assert_eq!(
        stages.mesh_changes_queued - stages.mesh_changes_dequeued,
        stages.mesh_changes_pending as u64,
        "retry must preserve the cumulative queue accounting invariant"
    );

    assert_eq!(stream.pop_mesh_change().unwrap().key(), first);
    assert_eq!(stream.pop_mesh_change().unwrap().key(), second);
    assert!(stream.pop_mesh_change().is_none());
}

#[test]
fn urgent_mesh_change_preempts_queued_bulk_publication() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let bulk = SubChunkKey::new(0, 8, 0, 0);
    let urgent = SubChunkKey::new(0, 0, 0, 0);
    stream.mesh_changes.push(super::WorldMeshChange::Remove {
        key: bulk,
        generation: 1,
        dirty_since: Instant::now(),
        urgent: false,
        permit: None,
    });
    stream.resident.insert(urgent);
    stream.known_air.insert(urgent);
    stream.mark_dirty_exact_with_priority(urgent, Instant::now(), true);

    stream.dispatch_mesh_jobs([0.0; 3], 1);

    assert!(matches!(
        stream.pop_mesh_change(),
        Some(super::WorldMeshChange::Remove {
            key,
            urgent: true,
            ..
        }) if key == urgent
    ));
    assert_eq!(
        stream.pop_mesh_change().map(|change| change.key()),
        Some(bulk)
    );
}

#[test]
fn stale_mesh_revision_is_rejected() {
    let key = SubChunkKey::new(0, -1, 2, 3);
    let mut revisions = RevisionTracker::default();
    let old = revisions.mark_dirty(key, Instant::now());
    let current = revisions.mark_dirty(key, Instant::now());

    assert!(!revisions.is_current(key, old));
    assert!(revisions.is_current(key, current));
}

#[test]
fn mesh_completion_carries_current_palette_native_biome_record() {
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
    stream.authority.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84], &RAW_BIOMES),
    );
    let source = stream.authority.terrain().sub_chunk(key).unwrap();
    let biome_source = stream.authority.terrain().biome_storage(key).unwrap();
    let generation = stream.revisions.mark_dirty(key, Instant::now());
    stream.mesh_jobs.in_flight.insert(key, generation);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        stream.runtime_assets(),
        stream.network_id_mode(),
        &Neighbourhood::empty(),
        &source,
    );
    let biome = PackedBiomeRecord::from_storage(&biome_source, |id| id + 1_000);
    let tint_identity = stream.biome_tint_identity();

    stream.accept_mesh_completion(MeshCompletion {
        output_permit: None,
        _job_permit: None,
        key,
        revision: generation,
        source,
        biome_sources: biome_neighbourhood_with_center(Some(biome_source)),
        biome,
        tint_identity,
        mesh,
        dependency_mask: MeshDependencyMask::default(),
        light_halo: Default::default(),
        queue_wait: Duration::ZERO,
        dispatch_wait: Duration::ZERO,
        duration: Duration::ZERO,
        urgent: false,
    });

    let super::WorldMeshChange::Upsert { biome, .. } = stream.pop_mesh_change().unwrap() else {
        panic!("expected biome-bearing mesh update")
    };
    assert_eq!(biome.tint_index(0, 0, 0), Some(1_042));
}

#[test]
fn stale_biome_snapshot_cannot_publish_an_old_tint_record() {
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
        .commit_level_chunk(
            key.chunk(),
            DecodedLevelChunk::decode(
                -4,
                1,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../world/fixtures/uniform_non_air.bin"
                )),
                &RAW_IDS,
            ),
        )
        .unwrap();
    stream.authority.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84], &RAW_BIOMES),
    );
    let source = stream.authority.terrain().sub_chunk(key).unwrap();
    let old_biome = stream.authority.terrain().biome_storage(key).unwrap();
    let generation = stream.revisions.mark_dirty(key, Instant::now());
    stream.mesh_jobs.in_flight.insert(key, generation);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        stream.runtime_assets(),
        stream.network_id_mode(),
        &Neighbourhood::empty(),
        &source,
    );
    let old_record = PackedBiomeRecord::from_storage(&old_biome, |_| 0);

    stream.authority.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 86], &RAW_BIOMES),
    );
    let tint_identity = stream.biome_tint_identity();
    stream.accept_mesh_completion(MeshCompletion {
        output_permit: None,
        _job_permit: None,
        key,
        revision: generation,
        source,
        biome_sources: biome_neighbourhood_with_center(Some(old_biome)),
        biome: old_record,
        tint_identity,
        mesh,
        dependency_mask: MeshDependencyMask::default(),
        light_halo: Default::default(),
        queue_wait: Duration::ZERO,
        dispatch_wait: Duration::ZERO,
        duration: Duration::ZERO,
        urgent: false,
    });

    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.pop_mesh_change().is_none());
}

#[test]
fn changed_neighbour_biome_cannot_publish_a_stale_cross_chunk_blend() {
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
        .commit_level_chunk(
            key.chunk(),
            DecodedLevelChunk::decode(
                -4,
                1,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../world/fixtures/uniform_non_air.bin"
                )),
                &RAW_IDS,
            ),
        )
        .unwrap();
    for (chunk, id) in [(key.chunk(), 42), (ChunkKey::new(0, 1, 0), 43)] {
        stream.authority.commit_biome_column(
            chunk,
            DecodedBiomeColumn::decode(-4, 1, &[1, id * 2], &RAW_BIOMES),
        );
    }
    let source = stream.authority.terrain().sub_chunk(key).unwrap();
    let biome_sources = stream.biome_neighbourhood(key);
    let old_record =
        super::pack_biome_record(&biome_sources, &stream.resolved_biome_tints_snapshot());
    let generation = stream.revisions.mark_dirty(key, Instant::now());
    stream.mesh_jobs.in_flight.insert(key, generation);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        stream.runtime_assets(),
        stream.network_id_mode(),
        &Neighbourhood::empty(),
        &source,
    );

    stream.authority.commit_biome_column(
        ChunkKey::new(0, 1, 0),
        DecodedBiomeColumn::decode(-4, 1, &[1, 88], &RAW_BIOMES),
    );
    stream.accept_mesh_completion(MeshCompletion {
        output_permit: None,
        _job_permit: None,
        key,
        revision: generation,
        source,
        biome_sources,
        biome: old_record,
        tint_identity: stream.biome_tint_identity(),
        mesh,
        dependency_mask: MeshDependencyMask::default(),
        light_halo: Default::default(),
        queue_wait: Duration::ZERO,
        dispatch_wait: Duration::ZERO,
        duration: Duration::ZERO,
        urgent: false,
    });

    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.pop_mesh_change().is_none());
}

/// Builds current worker completions held in staging by a zero publication allowance.
fn stream_with_staged_meshes(count: usize) -> (WorldStream, render_api::PublicationAllowance) {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let keys = (0..count)
        .map(|x| SubChunkKey::new(0, x as i32 * 2, -4, 0))
        .collect::<Vec<_>>();
    for &key in &keys {
        stream
            .authority
            .update_block(
                key,
                BlockUpdate::new(0, 0, 0, 0, 99),
                protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            )
            .unwrap();
        stream.resident.insert(key);
    }
    stream.mark_light_changed_sources(keys.iter().copied());
    light_scheduler::settle_light(&mut stream, [0.0; 3]);
    for &key in &keys {
        stream.mark_dirty_exact(key, Instant::now());
    }
    let config = render_api::PublicationServiceConfig::DEFAULT;
    let allowance = render_api::PublicationAllowance::new(config);
    allowance.begin_frame(1, 0, 0, 0, 0);
    stream.set_publication_allowance(allowance.clone());

    let mut dispatched = 0;
    test_time::eventually_within(
        std::time::Duration::from_secs(10),
        "mesh jobs to dispatch and drain",
        || {
            dispatched += stream.poll([0.0; 3], 32).mesh_jobs_dispatched;
            dispatched > 0
                && stream.mesh_jobs.in_flight.is_empty()
                && stream.mesh_jobs.pending.is_empty()
        },
    );
    for _ in 0..4 {
        dispatched += stream.poll([0.0; 3], 32).mesh_jobs_dispatched;
    }
    assert_eq!(
        dispatched, count,
        "a denied permit must not remesh the same revision"
    );
    assert_eq!(stream.staged_mesh_completions.len(), count);
    assert!(stream.take_mesh_changes().is_empty());

    (stream, allowance)
}

/// A staged mesh retains publication capacity while ready air fills a pressured frame.
#[test]
fn permit_denied_mesh_publishes_from_staging_amid_air_pressure() {
    let (mut stream, allowance) = stream_with_staged_meshes(1);
    let key = stream.staged_mesh_completions.front().unwrap().key;
    let config = render_api::PublicationServiceConfig::DEFAULT;
    for x in 10..18 {
        stream.mark_dirty_exact(SubChunkKey::new(0, x, 8, 0), Instant::now());
    }

    allowance.begin_frame(2, 8, config.maximum_frame_bytes, 8, 8);
    let dispatched = stream.poll([0.0; 3], 32).mesh_jobs_dispatched;
    let changes = stream.take_mesh_changes();
    assert_eq!(dispatched, 0);
    assert!(stream.staged_mesh_completions.is_empty());
    assert!(changes.iter().any(|change| matches!(
        change,
        WorldMeshChange::Upsert { key: published, permit: Some(_), .. } if *published == key
    )));
    assert!(
        changes
            .iter()
            .any(|change| matches!(change, WorldMeshChange::Remove { .. }))
    );
    assert!(changes.len() <= 8);
}

/// A staged empty result cannot hold later geometry behind sustained fresh removals.
#[test]
fn staged_empty_front_does_not_block_payloads_under_air_pressure() {
    let (mut stream, allowance) = stream_with_staged_meshes(2);
    let keys = stream
        .staged_mesh_completions
        .iter()
        .map(|completion| completion.key)
        .collect::<BTreeSet<_>>();
    let empty = stream.staged_mesh_completions.front_mut().unwrap();
    let old_bytes = chunk_publication_byte_len(&empty.mesh, &empty.biome);
    empty.mesh = ChunkMesh::default();
    let new_bytes = chunk_publication_byte_len(&empty.mesh, &empty.biome);
    stream.staged_mesh_bytes -= old_bytes - new_bytes;
    let config = render_api::PublicationServiceConfig::DEFAULT;
    for x in 10..10 + config.maximum_zero_byte_operations_per_frame as i32 {
        stream.mark_dirty_exact(SubChunkKey::new(0, x, 8, 0), Instant::now());
    }
    allowance.begin_frame(
        2,
        config.maximum_frame_items,
        config.maximum_frame_bytes,
        config.maximum_zero_byte_operations_per_frame,
        config.maximum_frame_items,
    );
    stream.poll([0.0; 3], 32);
    assert!(stream.staged_mesh_completions.is_empty());
    let changes = stream.take_mesh_changes();
    let published = changes
        .iter()
        .filter_map(|change| match change {
            WorldMeshChange::Upsert { key, .. } => Some(*key),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(published, keys);
    assert!(changes.len() <= config.maximum_frame_items);
}

/// An urgent retry publishes before ordinary air even when that ordinary section is closer.
#[test]
fn urgent_mesh_completion_retry_precedes_ordinary_air() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let calm = SubChunkKey::new(0, 0, 0, 0);
    stream.mark_dirty_exact(calm, Instant::now());
    let key = SubChunkKey::new(0, 64, 0, 0);
    let revision = stream.mark_dirty_exact(key, Instant::now());
    stream.mesh_jobs.pending.remove(&key);
    stream.mesh_jobs.scan.clear();

    stream.requeue_current_mesh_completion(key, revision, true);

    stream.dispatch_mesh_jobs_with_limits([0.0; 3], 0, 1);
    let changes = stream.take_mesh_changes();
    assert!(matches!(
        changes.as_slice(),
        [WorldMeshChange::Remove { key: published, generation, urgent: true, .. }]
            if *published == key && *generation == revision
    ));
    assert!(stream.mesh_jobs.pending.contains_key(&calm));
}
