use super::*;

/// Builds the standard focused world stream used by wire-preemption tests.
fn wire_test_stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    })
}

/// Builds one deterministic truncated chunk decode error.
fn truncated_chunk_error() -> world::DecodeError {
    world::DecodeError::UnexpectedEof {
        context: "wire-preemption witness",
        needed: 1,
        remaining: 0,
    }
}

/// Builds one deterministic truncated block-entity decode error.
fn truncated_block_entity_error() -> world::BlockEntityError {
    world::BlockEntityNbtError::UnexpectedEof {
        context: "wire-preemption witness",
        needed: 1,
        remaining: 0,
    }
    .into()
}

#[test]
fn malformed_inactive_inline_chunk_preempts_semantic_scope_gate() {
    let mut stream = wire_test_stream();
    stream.apply_prepared_with_sequence(
        PreparedWorldEvent::InlineLevelChunk {
            event: LevelChunkEvent {
                dimension: 0,
                x: 100,
                z: 100,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: Vec::new(),
            },
            decoded: Err(truncated_chunk_error()),
            duration: Duration::ZERO,
        },
        Some(7),
    );

    assert!(matches!(
        stream.take_fatal_error(),
        Some(WorldStreamFatalError::ChunkDecode { sequence: 7, .. })
    ));
}

#[test]
fn malformed_unexpected_subchunk_preempts_expectedness_gate() {
    let mut stream = wire_test_stream();
    stream.apply_prepared_with_sequence(
        PreparedWorldEvent::SubChunks {
            dimension: 0,
            entries: vec![PreparedSubChunk {
                position: [0, -4, 0],
                result: PreparedSubChunkResult::Decoded(Err(truncated_chunk_error())),
            }],
            duration: Duration::ZERO,
        },
        Some(8),
    );

    assert!(matches!(
        stream.take_fatal_error(),
        Some(WorldStreamFatalError::ChunkDecode { sequence: 8, .. })
    ));
}

#[test]
fn malformed_inactive_block_actor_data_preempts_scope_gate() {
    let mut stream = wire_test_stream();
    stream.apply_prepared_with_sequence(
        PreparedWorldEvent::BlockEntityUpdate {
            key: BlockEntityKey::new(0, 1_600, 0, 1_600),
            decoded: Err(truncated_block_entity_error()),
            duration: Duration::ZERO,
        },
        Some(9),
    );

    assert!(matches!(
        stream.take_fatal_error(),
        Some(WorldStreamFatalError::ChunkDecode { sequence: 9, .. })
    ));
}

#[test]
fn later_malformed_subchunk_prevents_valid_prefix_mutation() {
    let (mut stream, keys, _) = stream_with_unsent_sub_chunks(2);
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![
                    SubChunkEntryEvent {
                        position: [keys[0].x, keys[0].y, keys[0].z],
                        result: SubChunkResult::AllAir,
                    },
                    SubChunkEntryEvent {
                        position: [keys[1].x, keys[1].y, keys[1].z],
                        result: SubChunkResult::Success {
                            payload: vec![0xff],
                        },
                    },
                ],
            }),
        )
        .expect("admit multi-entry SubChunk batch");

    complete_pending_decode_jobs(&mut stream);

    assert!(matches!(
        stream.take_fatal_error(),
        Some(WorldStreamFatalError::ChunkDecode { sequence: 2, .. })
    ));
    assert!(!stream.known_air.contains(&keys[0]));
    assert!(!stream.store.is_sub_chunk_loaded(keys[0]));
}

#[test]
fn semantic_first_truncated_live_nbt_is_wire_fatal() {
    let mut stream = wire_test_stream();
    stream
        .submit(
            1,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position: [0, 0, 0],
                // Compound root, then `id` with the wrong Int type and no Int payload.
                nbt: vec![10, 0, 3, 2, b'i', b'd'],
            }),
        )
        .expect("admit semantic-first malformed NBT");
    complete_pending_decode_jobs(&mut stream);

    assert!(matches!(
        stream.take_fatal_error(),
        Some(WorldStreamFatalError::ChunkDecode { sequence: 1, .. })
    ));
}

#[test]
fn fully_structured_wrong_type_live_nbt_remains_semantic() {
    let mut stream = wire_test_stream();
    stream
        .submit(
            1,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: 0,
                position: [0, 0, 0],
                // The wrong Int-typed `id` has a complete zero payload and compound end.
                nbt: vec![10, 0, 3, 2, b'i', b'd', 0, 0],
            }),
        )
        .expect("admit complete semantic NBT shape");
    complete_pending_decode_jobs(&mut stream);

    assert!(stream.take_fatal_error().is_none());
    assert_eq!(stream.stats().decode_errors, 1);
}

#[test]
fn post_fatal_transport_callbacks_only_drain_prior_accounting() {
    let (mut stream, _, request) = stream_with_unsent_sub_chunks(1);
    stream.record_sub_chunk_request_transport_pending(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
    );
    assert_eq!(stream.transport_pending_requests, 1);
    let sent_before = stream.stats().phase2_stages.requests_sent;
    stream.apply_prepared_with_sequence(
        PreparedWorldEvent::BlockEntityUpdate {
            key: BlockEntityKey::new(0, 0, 0, 0),
            decoded: Err(truncated_block_entity_error()),
            duration: Duration::ZERO,
        },
        Some(2),
    );

    assert_eq!(stream.transport_pending_requests, 1);
    stream.record_sub_chunk_request_transport_pending(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
    );
    assert_eq!(stream.transport_pending_requests, 1);
    stream.acknowledge_sub_chunk_request_sent(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
        Instant::now(),
    );
    stream.acknowledge_sub_chunk_request_sent(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
        Instant::now(),
    );

    assert_eq!(stream.transport_pending_requests, 0);
    assert_eq!(stream.stats().phase2_stages.requests_sent, sent_before);
    assert!(stream.sub_chunk_deadlines.is_empty());
    assert!(stream.take_requests().is_empty());
    assert!(stream.committed_view_cohort.is_none());
}

#[test]
fn in_flight_mesh_completion_is_accounted_but_not_published_after_fatal() {
    let mut stream = wire_test_stream();
    let key = SubChunkKey::new(0, 0, -4, 0);
    let decoded = DecodedLevelChunk::decode(
        -4,
        1,
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../world/fixtures/uniform_non_air.bin"
        )),
    )
    .expect("decode mesh completion source");
    stream
        .store
        .commit_level_chunk(key.chunk(), decoded)
        .expect("commit mesh completion source");
    let source = stream.store.sub_chunk(key).expect("mesh source");
    stream.resident.insert(key);
    let revision = stream.revisions.mark_dirty(key, Instant::now());
    stream.in_flight.insert(key, revision);
    let mesh = mesh_sub_chunk(
        &stream.classifier,
        &stream.runtime_assets,
        stream.network_id_mode,
        &Neighbourhood::empty(),
        &source,
    );
    stream.pending_mesh.insert(
        SubChunkKey::new(0, 1, -4, 0),
        PendingMesh {
            revision: 1,
            since: Instant::now(),
            queued_at: Instant::now(),
            urgent: false,
        },
    );
    stream.mesh_changes.push_back(WorldMeshChange::Remove {
        key: SubChunkKey::new(0, 2, -4, 0),
        generation: 1,
        dirty_since: Instant::now(),
        urgent: false,
        permit: None,
    });
    stream
        .mesh_tx
        .send(MeshCompletion {
            key,
            revision,
            source,
            biome_sources: biome_neighbourhood_with_center(None),
            biome: PackedBiomeRecord::fallback(),
            tint_identity: stream.biome_tint_identity(),
            mesh,
            dependency_mask: MeshDependencyMask::default(),
            light_halo: Default::default(),
            queue_wait: Duration::ZERO,
            duration: Duration::ZERO,
            urgent: false,
        })
        .expect("queue in-flight mesh completion");
    stream.apply_prepared_with_sequence(
        PreparedWorldEvent::BlockEntityUpdate {
            key: BlockEntityKey::new(0, 0, 0, 0),
            decoded: Err(truncated_block_entity_error()),
            duration: Duration::ZERO,
        },
        Some(1),
    );

    let report = stream.poll([0.0; 3], 4);

    assert_eq!(report.mesh_results, 1);
    assert!(stream.in_flight.is_empty());
    assert!(stream.pending_mesh.is_empty());
    assert!(stream.mesh_changes.is_empty());
    assert_eq!(report.mesh_jobs_dispatched, 0);
}
