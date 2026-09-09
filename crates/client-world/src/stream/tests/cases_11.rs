use super::*;

const PLAYER_POSITION: [f32; 3] = [-511.0, 43.62, -511.0];
const PLAYER_COLUMN: [i32; 2] = [-32, -32];

fn inline_air_event_at(dimension: i32, x: i32, z: i32) -> WorldEvent {
    let range = protocol::vanilla_dimension_range(dimension)
        .expect("test dimension should have a vanilla range");
    let base_y = i8::try_from(range.base_sub_chunk_y)
        .expect("test dimension base sub-chunk Y fits the inline wire field");
    let mut payload = vec![9, 0, base_y as u8];
    payload.extend(biome_payload(dimension, 1));
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension,
        x,
        z,
        mode: LevelChunkMode::Inline { count: 1 },
        payload,
    })
}

fn stream_after_publisher_shrink() -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: PLAYER_POSITION,
        world_spawn_position: [-511, 43, -511],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(8))
        .expect("admit confirmed player-grid radius");
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [-511, 42, -513],
                radius_blocks: 128,
            }),
        )
        .expect("admit initial publisher view");
    stream
        .submit(
            3,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [-512, 64, -512],
                radius_blocks: 32,
            }),
        )
        .expect("admit publisher shrink");
    stream
}

#[test]
fn confirmed_player_grid_admits_inline_column_after_publisher_shrinks() {
    let mut stream = stream_after_publisher_shrink();
    let key = ChunkKey::new(0, -28, -32);
    assert_eq!(PLAYER_COLUMN, [-32, -32]);
    assert!(world::chunk_in_view(8, [key.x, key.z], PLAYER_COLUMN));
    assert!(!stream.column_is_active(key));

    let inactive_before = stream.stats().normalization_reasons.inactive_inline_chunks;
    stream
        .submit(4, inline_air_event_at(key.dimension, key.x, key.z))
        .expect("admit inline announcement into the decode FIFO");
    assert!(!stream.loaded_columns.contains(&key));
    assert!(!stream.required_columns().contains(&key));

    complete_pending_decode_jobs(&mut stream);
    stream.poll(PLAYER_POSITION, 0);

    // Publisher updates move the subscriber view, while successfully decoded
    // ordinary columns within the independently confirmed player grid remain
    // eligible world data and readiness members.
    assert_eq!(
        stream.stats().normalization_reasons.inactive_inline_chunks,
        inactive_before,
        "a player-grid column must not be rejected as inactive publisher data",
    );
    assert!(stream.loaded_columns.contains(&key));
    assert!(stream.required_columns().contains(&key));
    assert!(stream.resident.contains(&SubChunkKey::from_chunk(key, -4)));

    let target = stream
        .committed_view_cohort()
        .expect("publisher shrink commits a cohort identity");
    assert_eq!(
        target.publisher_geometry,
        Some(super::PublisherViewGeometry {
            center_blocks: [-512, -512],
            radius_blocks: 32,
        })
    );
    assert_eq!(stream.publisher_center, Some([-512, 64, -512]));
    assert_eq!(stream.publisher_radius_blocks, Some(32));
    assert_eq!(stream.publisher_radius_chunks, Some(2));

    let status = stream.cohort_status(target);
    assert_eq!(status.expected, 1);
    assert_eq!(status.loaded_target, 1);
    assert_eq!(status.missing_target, 0);
    assert_eq!(status.foreign_loaded, 0);
    assert_eq!(status.foreign_resident, 0);
    assert!(status.is_exact());

    let first_hash = status.required_hash;
    let second = ChunkKey::new(0, -28, -31);
    stream
        .submit(5, inline_air_event_at(second.dimension, second.x, second.z))
        .expect("admit a second player-grid announcement");
    complete_pending_decode_jobs(&mut stream);
    let expanded = stream.cohort_status(target);
    assert_eq!(expanded.expected, 2);
    assert_eq!(expanded.loaded_target, 2);
    assert_ne!(expanded.required_hash, first_hash);
    assert!(expanded.is_exact());
}

#[test]
fn request_and_subchunk_completion_use_confirmed_player_grid_interest() {
    let mut stream = stream_after_publisher_shrink();
    let key = ChunkKey::new(0, -28, -32);
    stream
        .submit(
            4,
            request_level_chunk_event(
                key.dimension,
                key.x,
                key.z,
                LevelChunkMode::LimitedRequests { highest: 1 },
                1,
            ),
        )
        .expect("admit request-mode player-grid announcement");
    complete_pending_decode_jobs(&mut stream);

    assert!(stream.required_columns().contains(&key));
    assert!(stream.requested_sub_chunks.contains_key(&key));
    let request = stream
        .pop_next_request()
        .expect("request-mode announcement queues its sub-chunk request");
    acknowledge_request_sent(&mut stream, &request, Instant::now());
    let sub_chunk = SubChunkKey::from_chunk(key, request.base_sub_chunk_y);
    stream
        .submit(
            5,
            WorldEvent::SubChunkReplyAdmission(SubChunkReplyAdmissionEvent {
                dimension: key.dimension,
                positions: vec![[sub_chunk.x, sub_chunk.y, sub_chunk.z]],
            }),
        )
        .expect("admit correlated reply metadata in player-grid scope");
    stream
        .submit(
            6,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: key.dimension,
                entries: vec![SubChunkEntryEvent {
                    position: [sub_chunk.x, sub_chunk.y, sub_chunk.z],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .expect("admit correlated player-grid reply");
    complete_pending_decode_jobs(&mut stream);
    stream.poll(PLAYER_POSITION, 0);

    assert_eq!(stream.stats().phase2_stages.responses_admitted, 1);
    assert_eq!(stream.stats().phase2_outcomes.stale, 0);
    assert_eq!(stream.stats().phase2_outcomes.all_air, 1);
    assert!(stream.loaded_columns.contains(&key));
    assert!(stream.resident.contains(&sub_chunk));

    stream
        .submit(
            7,
            WorldEvent::ChunkResync(ChunkResyncEvent {
                dimension: key.dimension,
                x: key.x,
                z: key.z,
                requested_sub_chunks: None,
                requested_sub_chunk_ys: Some(vec![sub_chunk.y]),
            }),
        )
        .expect("admit player-grid recovery");
    let recovery = stream
        .pop_next_request()
        .expect("player-grid recovery queues its exact request");
    assert_eq!(recovery.chunk, key);
    assert_eq!(
        (recovery.base_sub_chunk_y, recovery.count),
        (sub_chunk.y, 1)
    );
}

#[test]
fn live_mutations_use_confirmed_player_grid_interest() {
    let mut stream = stream_after_publisher_shrink();
    let key = ChunkKey::new(0, -28, -32);
    stream
        .submit(4, inline_air_event_at(key.dimension, key.x, key.z))
        .expect("admit player-grid column");
    complete_pending_decode_jobs(&mut stream);

    let inactive_blocks = stream.stats().normalization_reasons.inactive_block_updates;
    stream
        .submit(
            5,
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: key.dimension,
                position: [key.x * 16, -64, key.z * 16],
                layer: 0,
                network_id: 1,
            }]),
        )
        .expect("admit player-grid block update");
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream.stats().normalization_reasons.inactive_block_updates,
        inactive_blocks
    );
    assert!(
        stream
            .store
            .sub_chunk(SubChunkKey::from_chunk(key, -4))
            .is_some()
    );

    let inactive_actors = stream
        .stats()
        .normalization_reasons
        .inactive_block_entity_updates;
    let position = [key.x * 16, -64, key.z * 16];
    let block_entity = BlockEntityKey::new(key.dimension, position[0], position[1], position[2]);
    stream
        .submit(
            6,
            WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                dimension: key.dimension,
                position,
                nbt: block_entity_nbt("Chest", position),
            }),
        )
        .expect("admit player-grid block-entity update");
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream
            .stats()
            .normalization_reasons
            .inactive_block_entity_updates,
        inactive_actors
    );
    assert_eq!(
        stream
            .store
            .block_entity(block_entity)
            .expect("player-grid block entity commits to the world store")
            .id(),
        Some("Chest")
    );
}

#[test]
fn confirmed_player_grid_interest_uses_clamped_radius_and_bounded_edges() {
    let mut stream = stream_after_publisher_shrink();
    stream
        .submit(4, WorldEvent::ChunkRadiusUpdated(i32::MAX))
        .expect("admit oversized confirmed radius");
    assert_eq!(
        stream.chunk_radius,
        Some(super::PHASE0_MAX_VIEW_RADIUS_CHUNKS)
    );

    let edge = ChunkKey::new(
        0,
        PLAYER_COLUMN[0] + super::PHASE0_MAX_VIEW_RADIUS_CHUNKS + 2,
        PLAYER_COLUMN[1],
    );
    let outside = ChunkKey::new(0, edge.x + 1, edge.z);
    assert!(stream.column_is_data_interesting(edge));
    assert!(!stream.column_is_data_interesting(outside));
    assert!(!stream.column_is_data_interesting(ChunkKey::new(1, edge.x, edge.z)));
    assert!(!stream.column_is_data_interesting(ChunkKey::new(0, i32::MAX, i32::MIN)));
}

#[test]
fn unannounced_player_grid_data_remains_foreign_to_publisher_readiness() {
    let mut stream = stream_after_publisher_shrink();
    let announced = ChunkKey::new(0, -28, -32);
    stream
        .submit(
            4,
            inline_air_event_at(announced.dimension, announced.x, announced.z),
        )
        .expect("admit announced player-grid column");
    complete_pending_decode_jobs(&mut stream);

    let unannounced = SubChunkKey::new(0, -27, -4, -32);
    stream.record_known_air(unannounced);
    stream.loaded_columns.insert(unannounced.chunk());
    let target = stream.committed_view_cohort().unwrap();
    let status = stream.cohort_status(target);
    assert_eq!(status.expected, 1);
    assert_eq!(status.loaded_target, 1);
    assert_eq!(status.foreign_loaded, 1);
    assert_eq!(status.foreign_resident, 1);
    assert!(!status.is_exact());
}

#[test]
fn inline_column_outside_publisher_and_player_grid_remains_inactive() {
    let mut stream = stream_after_publisher_shrink();
    let key = ChunkKey::new(0, -20, -32);
    assert!(!world::chunk_in_view(8, [key.x, key.z], PLAYER_COLUMN));
    assert!(!stream.column_is_active(key));

    stream
        .submit(4, inline_air_event_at(key.dimension, key.x, key.z))
        .expect("admit bounded negative witness into the decode FIFO");
    complete_pending_decode_jobs(&mut stream);
    stream.poll(PLAYER_POSITION, 0);

    assert!(!stream.loaded_columns.contains(&key));
    assert!(!stream.required_columns().contains(&key));
    assert_eq!(
        stream.stats().normalization_reasons.inactive_inline_chunks,
        1
    );
    assert!(stream.take_fatal_error().is_none());
}

#[test]
fn inline_column_in_another_dimension_remains_inactive() {
    let mut stream = stream_after_publisher_shrink();
    let key = ChunkKey::new(1, -28, -32);

    stream
        .submit(4, inline_air_event_at(key.dimension, key.x, key.z))
        .expect("admit supported other-dimension wire into the decode FIFO");
    complete_pending_decode_jobs(&mut stream);
    stream.poll(PLAYER_POSITION, 0);

    assert!(!stream.loaded_columns.contains(&key));
    assert!(!stream.required_columns().contains(&key));
    assert_eq!(
        stream.stats().normalization_reasons.inactive_inline_chunks,
        1
    );
    assert!(stream.take_fatal_error().is_none());
}

#[test]
fn malformed_inline_wire_outside_both_scopes_is_still_fatal() {
    let mut stream = stream_after_publisher_shrink();
    stream
        .submit(
            4,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: -20,
                z: -32,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: vec![0xff],
            }),
        )
        .expect("admit malformed wire into the decode FIFO");

    complete_pending_decode_jobs(&mut stream);
    stream.poll(PLAYER_POSITION, 0);

    assert!(matches!(
        stream.take_fatal_error(),
        Some(WorldStreamFatalError::ChunkDecode { sequence: 4, .. })
    ));
}

#[test]
fn ordinary_long_travel_prunes_evicted_required_history() {
    let mut stream = stream_after_publisher_shrink();
    let origin = ChunkKey::new(0, -28, -32);
    stream
        .submit(4, inline_air_event_at(origin.dimension, origin.x, origin.z))
        .expect("admit origin announcement");
    complete_pending_decode_jobs(&mut stream);
    let target = stream.committed_view_cohort().unwrap();
    let origin_hash = stream.cohort_status(target).required_hash;
    let mut previous = origin;
    let mut sequence = 5_u64;

    for step in 0..12 {
        let destination = ChunkKey::new(0, step * 32, 0);
        let destination_x = f32::from(i16::try_from(destination.x).unwrap()) * 16.0 + 0.5;
        stream
            .submit(
                sequence,
                WorldEvent::MovePlayer(MovePlayerEvent {
                    runtime_id: 1,
                    position: [destination_x, 43.62, 0.5],
                    mode: MovePlayerMode::Normal,
                    ..Default::default()
                }),
            )
            .expect("commit ordinary long-distance movement");
        sequence += 1;

        assert!(!stream.loaded_columns.contains(&previous));
        assert!(!stream.required_columns().contains(&previous));

        stream
            .submit(
                sequence,
                inline_air_event_at(destination.dimension, destination.x, destination.z),
            )
            .expect("admit destination player-grid announcement");
        sequence += 1;
        complete_pending_decode_jobs(&mut stream);

        let expected = BTreeSet::from([destination]);
        assert_eq!(stream.required_columns(), &expected);
        let status = stream.cohort_status(target);
        assert_eq!(
            status.required_hash,
            super::super::diagnostics::deterministic_chunk_key_hash(&expected)
        );
        assert_eq!(status.expected, 1);
        assert_eq!(status.loaded_target, 1);
        assert_eq!(status.missing_target, 0);
        assert!(status.is_exact());
        previous = destination;
    }

    assert_ne!(stream.cohort_status(target).required_hash, origin_hash);
    assert_eq!(stream.committed_view_cohort(), Some(target));
    assert_eq!(stream.publisher_epoch, 2);
    assert_eq!(stream.publisher_center, Some([-512, 64, -512]));
    assert_eq!(stream.publisher_radius_blocks, Some(32));
}

#[test]
fn confirmed_radius_shrink_prunes_outer_requirement_but_keeps_inner_pending() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, WorldEvent::ChunkRadiusUpdated(8)).unwrap();
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 70, 0],
                radius_blocks: 32,
            }),
        )
        .unwrap();
    let inner = ChunkKey::new(0, 3, 0);
    let outer = ChunkKey::new(0, 8, 0);
    stream
        .submit(
            3,
            request_level_chunk_event(
                inner.dimension,
                inner.x,
                inner.z,
                LevelChunkMode::LimitedRequests { highest: 1 },
                1,
            ),
        )
        .unwrap();
    stream
        .submit(4, inline_air_event_at(outer.dimension, outer.x, outer.z))
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let request = stream.pop_next_request().expect("inner request is queued");
    acknowledge_request_sent(&mut stream, &request, Instant::now());
    let deadlines_before = stream.sub_chunk_deadlines.clone();
    let confirmed_attempts_before =
        stream.requested_sub_chunks[&inner][&request.base_sub_chunk_y].confirmed_attempts;
    let target = stream.committed_view_cohort().unwrap();

    assert_eq!(stream.required_columns(), &BTreeSet::from([inner, outer]));
    assert!(stream.requested_sub_chunks.contains_key(&inner));
    assert!(!stream.loaded_columns.contains(&inner));
    assert!(stream.loaded_columns.contains(&outer));
    assert_eq!(confirmed_attempts_before, 1);
    assert_eq!(stream.sub_chunk_deadlines.len(), 1);

    stream.submit(5, WorldEvent::ChunkRadiusUpdated(2)).unwrap();

    assert_eq!(stream.required_columns(), &BTreeSet::from([inner]));
    assert!(stream.requested_sub_chunks.contains_key(&inner));
    assert!(!stream.loaded_columns.contains(&inner));
    assert_eq!(stream.sub_chunk_deadlines, deadlines_before);
    assert_eq!(
        stream.requested_sub_chunks[&inner][&request.base_sub_chunk_y].confirmed_attempts,
        confirmed_attempts_before
    );
    assert!(!stream.tracked_columns().contains(&outer));
    assert_eq!(stream.committed_view_cohort(), Some(target));
    assert_eq!(stream.publisher_center, Some([0, 70, 0]));
    assert_eq!(stream.publisher_radius_blocks, Some(32));
    let status = stream.cohort_status(target);
    assert_eq!(status.expected, 1);
    assert_eq!(status.loaded_target, 0);
    assert_eq!(status.missing_target, 1);
    assert!(!status.target_is_complete());
}
