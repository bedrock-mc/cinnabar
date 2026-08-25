use super::*;

#[test]
fn local_actor_motion_commits_as_a_control_event_and_foreign_motion_is_dropped() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let motion = |actor_runtime_id: u64| {
        WorldEvent::ActorMotion(ActorMotionEvent {
            actor_runtime_id,
            motion: [1.5, 0.25, -0.75],
            tick: 7,
        })
    };

    // A foreign actor's impulse has no velocity consumer yet.
    stream.submit(1, motion(2)).unwrap();
    stream.submit(2, motion(1)).unwrap();

    let controls = stream.take_committed_controls();
    let [super::CommittedControlEvent::LocalActorMotion { sequence, event }] = controls.as_slice()
    else {
        panic!("unexpected committed controls {controls:?}");
    };
    assert_eq!(*sequence, 2);
    assert_eq!(event.actor_runtime_id, 1);
    assert_eq!(event.motion, [1.5, 0.25, -0.75]);
    assert_eq!(event.tick, 7);
}

#[test]
fn respawn_commits_as_a_local_position_authority_change() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let respawn = RespawnEvent {
        position: [8.5, 71.620_01, -4.25],
        state: 1,
        runtime_entity_id: 1,
    };

    stream.submit(1, WorldEvent::Respawn(respawn)).unwrap();

    assert_eq!(
        stream.take_committed_controls(),
        vec![super::CommittedControlEvent::Respawn {
            sequence: 1,
            respawn,
            resolved: super::server_position::ResolvedServerPosition {
                position: respawn.position,
                surface_anchor: None,
            },
        }]
    );
}

#[test]
fn older_movement_correction_tick_cannot_rewind_newer_correction() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
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
        subject: MovementCorrectionSubject::Player,
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
fn vehicle_correction_subject_commits_nothing_and_cannot_advance_the_tick_guard() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let correction = |subject, tick| PlayerMovementCorrectionEvent {
        position: [50.0, 80.0, -50.0],
        delta: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        subject,
        on_ground: true,
        tick,
    };

    // A vehicle rewind has no local-player consumer yet: it must not resolve a
    // server position, commit a control, or advance the monotonic guard that
    // would silence a later ordinary player correction.
    stream
        .submit(
            1,
            WorldEvent::PlayerMovementCorrection(correction(
                MovementCorrectionSubject::Vehicle,
                200,
            )),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::PlayerMovementCorrection(correction(
                MovementCorrectionSubject::Player,
                100,
            )),
        )
        .unwrap();

    let controls = stream.take_committed_controls();
    let [
        super::CommittedControlEvent::PlayerMovementCorrection {
            sequence,
            correction,
            ..
        },
    ] = controls.as_slice()
    else {
        panic!("unexpected committed controls {controls:?}");
    };
    assert_eq!(*sequence, 2);
    assert_eq!(correction.tick, 100);
    assert_eq!(correction.subject, MovementCorrectionSubject::Player);
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
        local_player_unique_id: 1,
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
            urgent: false,
            permit: None,
        });
    stream
        .mesh_changes
        .push_back(super::WorldMeshChange::Remove {
            key: second,
            generation: 2,
            dirty_since: Instant::now(),
            urgent: false,
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
fn urgent_mesh_change_preempts_queued_bulk_publication() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let bulk = SubChunkKey::new(0, 8, 0, 0);
    let urgent = SubChunkKey::new(0, 0, 0, 0);
    stream
        .mesh_changes
        .push_back(super::WorldMeshChange::Remove {
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
fn render_publication_retry_and_eviction_preserve_diagnostic_identity_summary() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
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

    stream
        .mesh_changes
        .push_back(super::WorldMeshChange::Remove {
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
        urgent: false,
    });

    assert_eq!(stream.stats().stale_mesh_jobs, 1);
    assert!(stream.pop_mesh_change().is_none());
}

#[test]
fn remesh_latency_closes_only_when_the_exact_generation_is_applied() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
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
        local_player_unique_id: 1,
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
fn starved_publication_tokens_still_dispatch_when_nothing_is_in_flight() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
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
    stream.mark_light_changed_sources([key]);
    light_scheduler::settle_light(&mut stream, [0.0; 3]);
    stream.mark_dirty_exact(key, Instant::now());

    // Exhaust the frame publication window entirely: without the starved
    // dispatch floor this poll could never start meshing again.
    let config = crate::PublicationServiceConfig::PHASE2_GATE;
    let allowance = crate::PublicationAllowance::new(config);
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
        air_network_id: 12_530,
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
            .store
            .update_block(*key, BlockUpdate::new(0, 0, 0, 0, 99), 12_530)
            .unwrap();
        stream.resident.insert(*key);
    }
    stream.mark_light_changed_sources(keys.iter().copied());
    light_scheduler::settle_light(&mut stream, [0.0; 3]);
    for key in &keys {
        stream.mark_dirty_exact(*key, Instant::now());
    }
    assert_eq!(stream.pending_mesh.len(), 6);
    assert!(stream.in_flight.is_empty());

    // Exhaust the frame publication window exactly like the single-job
    // witness above: the starved floor is the only remaining admission
    // route through the real poll path.
    let config = crate::PublicationServiceConfig::PHASE2_GATE;
    let allowance = crate::PublicationAllowance::new(config);
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
        stream.in_flight.len(),
        4,
        "exactly the floored budget must enter the worker window"
    );
}

#[test]
fn starved_mesh_dispatch_floor_never_invents_work_with_an_empty_pending_queue() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    // Same exhausted publication window as the floor witness, but with no
    // pending mesh work anywhere: starvation must not fabricate dispatches.
    let config = crate::PublicationServiceConfig::PHASE2_GATE;
    let allowance = crate::PublicationAllowance::new(config);
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
    assert!(stream.in_flight.is_empty());
    assert!(stream.take_mesh_changes().is_empty());
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
        local_player_unique_id: 1,
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
        local_player_unique_id: 1,
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
    stream.pending_mesh.remove(&key);
    stream.pending_mesh_scan.clear();

    stream.requeue_current_mesh_completion(key, revision, true);

    assert!(stream.pending_mesh[&key].urgent);
    assert_eq!(stream.pending_mesh_scan.front(), Some(&(key, revision)));
}
