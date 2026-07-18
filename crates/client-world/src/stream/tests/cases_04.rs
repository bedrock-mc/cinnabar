use super::*;

#[test]
fn older_movement_correction_tick_cannot_rewind_newer_correction() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let correction = |tick, position| PlayerMovementCorrectionEvent {
        position,
        delta: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        on_ground: true,
        tick,
    };
    let newer = correction(100, [100.0, 80.0, 100.0]);
    let older = correction(99, [10.0, 70.0, 10.0]);

    stream
        .submit(1, WorldEvent::PlayerMovementCorrection(newer))
        .unwrap();
    stream
        .submit(2, WorldEvent::PlayerMovementCorrection(older))
        .unwrap();

    assert_eq!(
        stream.take_committed_controls(),
        vec![super::CommittedControlEvent::PlayerMovementCorrection {
            sequence: 1,
            correction: newer,
            resolved: super::server_position::ResolvedServerPosition {
                position: newer.position,
                surface_anchor: None,
            },
        }]
    );
}

#[test]
fn newer_update_waits_for_older_decode_and_wins() {
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
    let mut ordered = SequenceBuffer::new(1);
    ordered.insert(2, Action::Update).unwrap();
    assert!(ordered.pop_next().is_none(), "sequence two must wait");
    ordered.insert(1, Action::Decode(decoded)).unwrap();

    let mut store = ChunkStore::new();
    while let Some(action) = ordered.pop_next() {
        match action {
            Action::Decode(decoded) => {
                store
                    .commit_level_chunk(ChunkKey::new(0, 0, 0), decoded)
                    .unwrap();
            }
            Action::Update => {
                store
                    .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
                    .unwrap();
            }
        }
    }

    assert_eq!(
        store.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(99)
    );
}

#[test]
fn render_backpressure_retry_preserves_change_order_for_eventual_delivery() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let first = SubChunkKey::new(0, 1, 2, 3);
    let second = SubChunkKey::new(0, 4, 5, 6);
    stream
        .mesh_changes
        .push_back(super::WorldMeshChange::Remove {
            key: first,
            generation: 1,
            dirty_since: Instant::now(),
            permit: None,
        });
    stream
        .mesh_changes
        .push_back(super::WorldMeshChange::Remove {
            key: second,
            generation: 2,
            dirty_since: Instant::now(),
            permit: None,
        });
    stream.stats.phase2_stages.mesh_changes_queued = 2;

    let blocked = stream.pop_mesh_change().unwrap();
    stream.retry_mesh_change_front(blocked).unwrap();

    let stages = stream
        .phase2_publication_snapshot(ChunkKey::new(0, 0, 0))
        .stages;
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
fn render_publication_retry_and_eviction_preserve_diagnostic_identity_summary() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let key = SubChunkKey::new(0, 1, 2, 3);
    let source = uniform_sub_chunk(50_000);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        &stream.runtime_assets,
        stream.network_id_mode,
        &Neighbourhood::empty(),
        &source,
    );
    stream
        .mesh_changes
        .push_back(super::WorldMeshChange::Upsert {
            key,
            mesh,
            biome: PackedBiomeRecord::fallback(),
            tint_identity: stream.biome_tint_identity(),
            generation: 1,
            dirty_since: Instant::now(),
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

    stream
        .mesh_changes
        .push_back(super::WorldMeshChange::Remove {
            key,
            generation: 2,
            dirty_since: Instant::now(),
            permit: None,
        });
    assert!(matches!(
        stream.pop_mesh_change(),
        Some(super::WorldMeshChange::Remove { key: removed, .. }) if removed == key
    ));
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
    stream.store.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84]).unwrap(),
    );
    let source = stream.store.sub_chunk(key).unwrap();
    let biome_source = stream.store.biome_storage(key).unwrap();
    let generation = stream.revisions.mark_dirty(key, Instant::now());
    stream.in_flight.insert(key, generation);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        &stream.runtime_assets,
        stream.network_id_mode,
        &Neighbourhood::empty(),
        &source,
    );
    let biome = PackedBiomeRecord::from_storage(&biome_source, |id| id + 1_000);
    let tint_identity = stream.biome_tint_identity();

    stream.accept_mesh_completion(MeshCompletion {
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
        duration: Duration::ZERO,
    });

    let super::WorldMeshChange::Upsert { biome, .. } = stream.pop_mesh_change().unwrap() else {
        panic!("expected biome-bearing mesh update")
    };
    assert_eq!(biome.tint_index(0, 0, 0), Some(1_042));
}

#[test]
fn stale_biome_snapshot_cannot_publish_an_old_tint_record() {
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
        .commit_level_chunk(
            key.chunk(),
            DecodedLevelChunk::decode(
                -4,
                1,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../world/fixtures/uniform_non_air.bin"
                )),
            )
            .unwrap(),
        )
        .unwrap();
    stream.store.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 84]).unwrap(),
    );
    let source = stream.store.sub_chunk(key).unwrap();
    let old_biome = stream.store.biome_storage(key).unwrap();
    let generation = stream.revisions.mark_dirty(key, Instant::now());
    stream.in_flight.insert(key, generation);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        &stream.runtime_assets,
        stream.network_id_mode,
        &Neighbourhood::empty(),
        &source,
    );
    let old_record = PackedBiomeRecord::from_storage(&old_biome, |_| 0);

    stream.store.commit_biome_column(
        key.chunk(),
        DecodedBiomeColumn::decode(-4, 1, &[1, 86]).unwrap(),
    );
    let tint_identity = stream.biome_tint_identity();
    stream.accept_mesh_completion(MeshCompletion {
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
        duration: Duration::ZERO,
    });

    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.pop_mesh_change().is_none());
}

#[test]
fn changed_neighbour_biome_cannot_publish_a_stale_cross_chunk_blend() {
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
        .commit_level_chunk(
            key.chunk(),
            DecodedLevelChunk::decode(
                -4,
                1,
                include_bytes!(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../world/fixtures/uniform_non_air.bin"
                )),
            )
            .unwrap(),
        )
        .unwrap();
    for (chunk, id) in [(key.chunk(), 42), (ChunkKey::new(0, 1, 0), 43)] {
        stream.store.commit_biome_column(
            chunk,
            DecodedBiomeColumn::decode(-4, 1, &[1, id * 2]).unwrap(),
        );
    }
    let source = stream.store.sub_chunk(key).unwrap();
    let biome_sources = stream.biome_neighbourhood(key);
    let old_record =
        super::pack_biome_record(&biome_sources, &stream.resolved_biome_tints_snapshot());
    let generation = stream.revisions.mark_dirty(key, Instant::now());
    stream.in_flight.insert(key, generation);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        &stream.runtime_assets,
        stream.network_id_mode,
        &Neighbourhood::empty(),
        &source,
    );

    stream.store.commit_biome_column(
        ChunkKey::new(0, 1, 0),
        DecodedBiomeColumn::decode(-4, 1, &[1, 88]).unwrap(),
    );
    stream.accept_mesh_completion(MeshCompletion {
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
        duration: Duration::ZERO,
    });

    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.pop_mesh_change().is_none());
}

#[test]
fn remesh_latency_closes_only_when_the_exact_generation_is_applied() {
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
    let dirty_since = Instant::now();
    let generation = stream.revisions.mark_dirty(key, dirty_since);
    stream.resident.insert(key);
    assert_eq!(stream.unacknowledged_mesh_count(), 1);
    assert!(!stream.is_mesh_clean(key));
    stream
        .requested_sub_chunks
        .insert(key.chunk(), BTreeMap::from([(key.y, Default::default())]));
    assert_eq!(stream.outstanding_sub_chunk_count(), 1);
    stream.requested_sub_chunks.clear();
    stream.in_flight.insert(key, generation);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        &stream.runtime_assets,
        stream.network_id_mode,
        &Neighbourhood::empty(),
        source.as_ref(),
    );
    let tint_identity = stream.biome_tint_identity();
    stream.accept_mesh_completion(MeshCompletion {
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
        duration: std::time::Duration::from_millis(5),
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
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
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
    stats.observe_mesh_queue_wait(std::time::Duration::from_millis(13));
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
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
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

#[test]
fn forced_remesh_returns_exact_resident_generation_manifest() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let keys = [
        SubChunkKey::new(0, -1, -4, 2),
        SubChunkKey::new(0, 0, -4, 0),
        SubChunkKey::new(0, 1, -4, -2),
    ];
    for key in keys {
        stream
            .store
            .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
            .unwrap();
        stream.resident.insert(key);
    }
    let known_air = SubChunkKey::new(0, 2, -4, 3);
    stream.record_known_air(known_air);
    stream.mark_light_changed_sources(keys.into_iter().chain([known_air]));
    assert_eq!(stream.dispatch_light_jobs([0.0; 3], 4), 4);
    for _ in 0..4 {
        let completion = stream
            .light_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("forced-remesh setup light completion");
        stream.accept_light_completion(completion);
    }
    let previously_dirty_at = std::time::Instant::now();
    stream.mark_dirty_exact(keys[0], previously_dirty_at);
    let started = previously_dirty_at + Duration::from_millis(1);

    let manifest = stream.remesh_all_resident(started);

    assert_eq!(manifest.started_at, started);
    assert_eq!(manifest.entries.len(), 4);
    assert_eq!(
        manifest
            .entries
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>(),
        keys.into_iter().chain([known_air]).collect()
    );
    assert_eq!(
        manifest
            .entries
            .iter()
            .map(|(_, generation)| *generation)
            .collect::<BTreeSet<_>>()
            .len(),
        manifest.entries.len(),
        "every forced remesh key must receive one unique generation"
    );
    for (key, generation) in manifest.entries.iter().copied() {
        let dirty = stream.revisions.dirty(key).unwrap();
        assert_eq!(dirty.since, started);
        assert_eq!(dirty.revision, generation);
    }

    assert_eq!(stream.dispatch_mesh_jobs([0.0; 3], 3), 3);
    assert!(stream.take_mesh_changes().iter().any(|change| {
        matches!(
            change,
            super::WorldMeshChange::Remove { key, generation, dirty_since, .. }
                if *key == known_air
                    && manifest.entries.contains(&(*key, *generation))
                    && *dirty_since == started
        )
    }));
}

#[test]
fn forced_remesh_of_frozen_published_manifest_skips_unpublished_and_air_keys() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let published = [SubChunkKey::new(0, 0, -4, 0), SubChunkKey::new(0, 1, -4, 0)];
    let unpublished = SubChunkKey::new(0, 2, -4, 0);
    let known_air = SubChunkKey::new(0, 3, -4, 0);
    for key in published.into_iter().chain([unpublished]) {
        stream
            .store
            .update_block(key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
            .unwrap();
        stream.resident.insert(key);
    }
    stream.record_known_air(known_air);
    let frozen = Arc::<[(SubChunkKey, u64)]>::from([(published[0], 40), (published[1], 41)]);
    stream.applied_mesh_generations.insert(published[0], 40);
    stream.applied_mesh_generations.insert(published[1], 41);
    let resident_before = stream.resident.clone();
    let known_air_before = stream.known_air.clone();
    let started = Instant::now();

    let manifest = stream
        .remesh_published_manifest(&frozen, started)
        .expect("the exact frozen published manifest should remesh");

    assert_eq!(
        manifest
            .entries
            .iter()
            .map(|(key, _)| *key)
            .collect::<BTreeSet<_>>(),
        published.into_iter().collect()
    );
    assert!(manifest.entries.iter().all(|(key, generation)| {
        frozen
            .iter()
            .find(|(published_key, _)| published_key == key)
            .is_some_and(|(_, previous)| previous != generation)
    }));
    assert_eq!(
        stream.pending_mesh.keys().copied().collect::<BTreeSet<_>>(),
        published.into_iter().collect(),
        "unpublished resident and known-air identities must not create no-mesh jobs"
    );
    assert_eq!(stream.resident, resident_before);
    assert_eq!(stream.known_air, known_air_before);
    assert_eq!(
        stream.forced_remesh_manifest_state(&manifest),
        super::ForcedRemeshManifestState::Pending
    );
}

#[test]
fn published_manifest_remesh_rejects_stale_duplicate_or_nonresident_allocations() {
    let new_stream = || {
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
        stream.resident.insert(key);
        stream.applied_mesh_generations.insert(key, 7);
        (stream, key)
    };
    let now = Instant::now();

    let (mut stale, stale_key) = new_stream();
    assert!(
        stale
            .remesh_published_manifest(&[(stale_key, 6)], now)
            .is_none()
    );
    assert!(stale.pending_mesh.is_empty());

    let (mut duplicate, duplicate_key) = new_stream();
    assert!(
        duplicate
            .remesh_published_manifest(&[(duplicate_key, 7), (duplicate_key, 7)], now)
            .is_none()
    );
    assert!(duplicate.pending_mesh.is_empty());

    let (mut nonresident, nonresident_key) = new_stream();
    nonresident.resident.remove(&nonresident_key);
    assert!(
        nonresident
            .remesh_published_manifest(&[(nonresident_key, 7)], now)
            .is_none()
    );
    assert!(nonresident.pending_mesh.is_empty());

    let (mut known_air, known_air_key) = new_stream();
    known_air.record_known_air(known_air_key);
    assert!(
        known_air
            .remesh_published_manifest(&[(known_air_key, 7)], now)
            .is_none(),
        "a key that became known air must not create a forced removal job"
    );
    assert!(known_air.pending_mesh.is_empty());
}

#[test]
fn eviction_or_superseding_revision_cannot_complete_forced_manifest() {
    let new_stream = || {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        });
        let key = SubChunkKey::new(0, 0, -4, 0);
        stream.record_known_air(key);
        (stream, key)
    };

    let started = Instant::now();
    let (mut evicted, evicted_key) = new_stream();
    let evicted_manifest = evicted.remesh_all_resident(started);
    evicted.evict_column(evicted_key.chunk());
    assert_eq!(
        evicted.forced_remesh_manifest_state(&evicted_manifest),
        super::ForcedRemeshManifestState::Invalid
    );

    let (mut superseded, superseded_key) = new_stream();
    let superseded_manifest = superseded.remesh_all_resident(started);
    let superseded_at = started + Duration::from_millis(1);
    superseded.mark_dirty_exact(superseded_key, superseded_at);
    let replacement = superseded.revisions.dirty(superseded_key).unwrap();
    superseded.acknowledge_mesh_upload(
        superseded_key,
        replacement.revision,
        superseded_at,
        superseded_at + Duration::from_millis(1),
    );
    assert_eq!(
        superseded.forced_remesh_manifest_state(&superseded_manifest),
        super::ForcedRemeshManifestState::Invalid,
        "applying a replacement revision must not satisfy the forced generation"
    );
}

#[test]
fn negative_absolute_updates_use_euclidean_chunk_coordinates() {
    let event = BlockUpdateEvent {
        dimension: 2,
        position: [-1, -65, 16],
        layer: 1,
        network_id: 0xdead_beef,
    };
    let (key, update) = split_block_update(event).unwrap();

    assert_eq!(key, SubChunkKey::new(2, -1, -5, 1));
    assert_eq!(update, BlockUpdate::new(15, 15, 0, 1, 0xdead_beef));
}

#[test]
fn normalization_breakdown_distinguishes_inactive_and_malformed_world_traffic() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.chunk_radius = Some(0);

    let batches = stream.snapshot_block_mutation_batches(vec![
        BlockUpdateEvent {
            dimension: 0,
            position: [16, 0, 0],
            layer: 0,
            network_id: 1,
        },
        BlockUpdateEvent {
            dimension: 0,
            position: [0, 0, 0],
            layer: usize::MAX,
            network_id: 2,
        },
    ]);
    assert!(batches.is_empty());

    stream.apply_prepared(super::PreparedWorldEvent::SubChunks {
        dimension: 0,
        entries: vec![
            super::PreparedSubChunk {
                position: [0, 0, 0],
                result: super::PreparedSubChunkResult::AllAir,
            },
            super::PreparedSubChunk {
                position: [1, 0, 0],
                result: super::PreparedSubChunkResult::AllAir,
            },
        ],
        duration: std::time::Duration::ZERO,
    });

    let stats = stream.stats();
    assert_eq!(stats.normalization_errors, 3);
    assert_eq!(stats.normalization_reasons.inactive_block_updates, 1);
    assert_eq!(stats.normalization_reasons.malformed_block_updates, 1);
    assert_eq!(stats.normalization_reasons.unexpected_sub_chunks, 1);
    assert_eq!(stats.normalization_reasons.inactive_sub_chunks, 0);
    assert_eq!(stats.normalization_reasons.total(), 3);
}

#[test]
fn max_block_update_batch_prepares_off_thread_and_commits_atomically_in_fifo() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let mut updates = (0..4_095)
        .map(|linear| BlockUpdateEvent {
            dimension: 0,
            position: [linear >> 8, linear & 15, (linear >> 4) & 15],
            layer: 0,
            network_id: linear as u32 + 1,
        })
        .collect::<Vec<_>>();
    updates.push(BlockUpdateEvent {
        dimension: 0,
        position: [0, 0, 0],
        layer: 0,
        network_id: 99_999,
    });
    let movement = MovePlayerEvent {
        runtime_id: 1,
        position: [1.0, 70.0, 2.0],
        pitch: 0.0,
        yaw: 0.0,
        ..Default::default()
    };

    stream.submit(1, WorldEvent::BlockUpdates(updates)).unwrap();
    stream.submit(2, WorldEvent::MovePlayer(movement)).unwrap();

    assert_eq!(stream.stats().queued_decode_jobs, 1);
    assert!(stream.take_committed_controls().is_empty());
    assert!(
        stream
            .store
            .sub_chunk(SubChunkKey::new(0, 0, 0, 0))
            .is_none()
    );

    complete_pending_decode_jobs(&mut stream);

    let committed = stream
        .store
        .sub_chunk(SubChunkKey::new(0, 0, 0, 0))
        .unwrap();
    assert_eq!(committed.runtime_id(0, 0, 0, 0), Some(99_999));
    assert_eq!(committed.runtime_id(0, 15, 14, 15), Some(4_095));
    let key = SubChunkKey::new(0, 0, 0, 0);
    assert!(stream.block_generations.contains_key(&key));
    assert!(stream.pending_light.contains_key(&key));
    assert_eq!(
        stream.light_store.kind(key),
        world::LightSubChunkKind::Resident
    );
    assert_eq!(
        stream.take_committed_controls(),
        vec![super::CommittedControlEvent::MovePlayer {
            sequence: 2,
            movement,
            resolved: super::server_position::ResolvedServerPosition {
                position: movement.position,
                surface_anchor: None,
            },
            source_cohort: None,
        }]
    );
}
