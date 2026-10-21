use super::*;

#[test]
fn render_publication_retry_and_eviction_preserve_diagnostic_identity_summary() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 1, 2, 3);
    let source = uniform_sub_chunk(50_000);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        stream.runtime_assets(),
        stream.network_id_mode(),
        &Neighbourhood::empty(),
        &source,
    );
    stream.mesh_changes.push(super::WorldMeshChange::Upsert {
        output_permit: None,
        key,
        mesh,
        biome: PackedBiomeRecord::fallback(),
        tint_identity: stream.biome_tint_identity(),
        generation: 1,
        dirty_since: Instant::now(),
        urgent: false,
        permit: None,
    });

    let blocked = stream.pop_mesh_change().unwrap();
    stream.retry_mesh_change_front(blocked).unwrap();
    let super::WorldMeshChange::Upsert { mesh, .. } = stream.pop_mesh_change().unwrap() else {
        panic!("expected retried diagnostic upsert")
    };
    assert_eq!(
        mesh.diagnostic_geometry().entries(),
        &[::meshing::DiagnosticGeometryCount::new(None, 50_000, 96)]
    );

    stream.mesh_changes.push(super::WorldMeshChange::Remove {
        key,
        generation: 2,
        dirty_since: Instant::now(),
        urgent: false,
        permit: None,
    });
    assert!(matches!(
        stream.pop_mesh_change(),
        Some(super::WorldMeshChange::Remove { key: removed, .. }) if removed == key
    ));
}

#[test]
fn remesh_latency_closes_only_when_the_exact_generation_is_applied() {
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
    let dirty_since = Instant::now();
    let generation = stream.revisions.mark_dirty(key, dirty_since);
    stream.resident.insert(key);
    assert_eq!(stream.unacknowledged_mesh_count(), 1);
    assert!(!stream.is_mesh_clean(key));
    stream
        .requests
        .requested
        .insert(key.chunk(), BTreeMap::from([(key.y, Default::default())]));
    assert_eq!(stream.outstanding_sub_chunk_count(), 1);
    stream.requests.requested.clear();
    stream.mesh_jobs.in_flight.insert(key, generation);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        stream.runtime_assets(),
        stream.network_id_mode(),
        &Neighbourhood::empty(),
        source.as_ref(),
    );
    let tint_identity = stream.biome_tint_identity();
    stream.accept_mesh_completion(MeshCompletion {
        output_permit: None,
        _job_permit: None,
        key,
        revision: generation,
        source,
        biome_sources: biome_neighbourhood_with_center(None),
        biome: PackedBiomeRecord::fallback(),
        tint_identity,
        mesh,
        dependency_mask: MeshDependencyMask::default(),
        light_halo: Default::default(),
        queue_wait: Duration::ZERO,
        dispatch_wait: Duration::ZERO,
        duration: std::time::Duration::from_millis(5),
        urgent: false,
    });

    assert_eq!(
        stream.stats().max_remesh_latency,
        std::time::Duration::ZERO,
        "worker-ready mesh must not close update-to-visible latency"
    );
    let change = stream.pop_mesh_change().unwrap();
    let super::WorldMeshChange::Upsert {
        generation: queued_generation,
        dirty_since: queued_since,
        ..
    } = change
    else {
        panic!("expected queued mesh upload")
    };
    assert_eq!(queued_generation, generation);
    assert_eq!(queued_since, dirty_since);
    assert_eq!(stream.pending_mesh_change_count(), 0);

    let applied_at = dirty_since + std::time::Duration::from_millis(75);

    stream.acknowledge_mesh_upload(key, generation + 1, dirty_since, applied_at);
    assert_eq!(stream.stats().max_remesh_latency, std::time::Duration::ZERO);
    assert!(stream.revisions.is_current(key, generation));

    stream.acknowledge_mesh_upload(key, generation, dirty_since, applied_at);
    assert_eq!(
        stream.stats().max_remesh_latency,
        std::time::Duration::from_millis(75)
    );
    assert!(!stream.revisions.is_current(key, generation));
    assert_eq!(stream.unacknowledged_mesh_count(), 0);
    assert!(stream.is_mesh_clean(key));
}

#[test]
fn timed_session_resets_pre_ready_duration_high_water_marks_only() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream.stats.max_decode_duration = std::time::Duration::from_secs(3);
    stream.stats.max_mesh_duration = std::time::Duration::from_secs(4);
    stream.stats.max_remesh_latency = std::time::Duration::from_secs(12);
    stream.stats.decode_errors = 7;

    stream.begin_timed_session();

    assert_eq!(
        stream.stats().max_decode_duration,
        std::time::Duration::ZERO
    );
    assert_eq!(stream.stats().max_mesh_duration, std::time::Duration::ZERO);
    assert_eq!(stream.stats().max_remesh_latency, std::time::Duration::ZERO);
    assert_eq!(stream.stats().decode_errors, 7);
}

#[test]
fn publication_stage_queue_wait_excludes_worker_duration_and_maxima_do_not_shrink() {
    let queued_at = Instant::now();
    let started_at = queued_at + std::time::Duration::from_millis(17);
    let finished_at = started_at + std::time::Duration::from_millis(29);
    let mut stats = super::WorldStreamStats::default();

    stats.observe_decode_queue_wait(super::queue_wait(queued_at, started_at));
    stats.observe_decode_queue_wait(std::time::Duration::from_millis(3));
    stats.observe_light_queue_wait(std::time::Duration::from_millis(11));
    stats.observe_mesh_queue_wait(
        std::time::Duration::from_millis(13),
        std::time::Duration::ZERO,
    );
    stats.max_decode_duration = stats
        .max_decode_duration
        .max(finished_at.saturating_duration_since(started_at));
    stats.max_decode_duration = stats
        .max_decode_duration
        .max(std::time::Duration::from_millis(31));
    stats.max_light_duration = stats
        .max_light_duration
        .max(std::time::Duration::from_millis(23));
    stats.max_mesh_duration = stats
        .max_mesh_duration
        .max(std::time::Duration::from_millis(19));

    assert_eq!(
        stats.max_decode_queue_wait,
        std::time::Duration::from_millis(17)
    );
    assert_eq!(
        stats.max_decode_duration,
        std::time::Duration::from_millis(31)
    );
    assert_eq!(
        stats.max_light_queue_wait,
        std::time::Duration::from_millis(11)
    );
    assert_eq!(
        stats.max_light_duration,
        std::time::Duration::from_millis(23)
    );
    assert_eq!(
        stats.max_mesh_queue_wait,
        std::time::Duration::from_millis(13)
    );
    assert_eq!(
        stats.max_mesh_duration,
        std::time::Duration::from_millis(19)
    );
    assert_eq!(
        super::queue_wait(started_at, queued_at),
        std::time::Duration::ZERO,
        "an out-of-order clock observation must saturate at zero"
    );
}

#[test]
fn mesh_ack_diagnostic_retains_latest_timestamp_when_acks_arrive_out_of_order() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let started = Instant::now();
    let newer_key = SubChunkKey::new(0, 0, 0, 0);
    let older_key = SubChunkKey::new(0, 0, 1, 0);
    let newer_generation = stream.revisions.mark_dirty(newer_key, started);
    let older_generation = stream.revisions.mark_dirty(older_key, started);
    let newest = started + std::time::Duration::from_millis(100);
    let older = started + std::time::Duration::from_millis(50);

    stream.acknowledge_mesh_upload(newer_key, newer_generation, started, newest);
    stream.acknowledge_mesh_upload(older_key, older_generation, started, older);

    assert_eq!(stream.stats().last_mesh_ack_at, Some(newest));
}
