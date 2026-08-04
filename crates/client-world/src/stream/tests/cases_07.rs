use super::*;

#[test]
fn publisher_radius_classifier_is_policy_not_universal_wire_geometry() {
    let radius_120 = super::ViewCohort::from_publisher(0, [-1350, 104, 1634], 120);
    let radius_128 = super::ViewCohort::from_publisher(0, [-1350, 104, 1634], 128);
    let radius_256 = super::ViewCohort::from_publisher(0, [-1350, 104, 1634], 256);

    // Raw 120 is retained as an explicit compatibility policy. Dragonfly emits
    // multiples of sixteen and its attributable round-distance classifier is
    // 177 columns at raw 128 and 749 columns at raw 256. None of these counts
    // is asserted as universal NetworkChunkPublisherUpdate wire geometry.
    assert_eq!(radius_120.classifier_columns().len(), 177);
    assert_eq!(radius_128.classifier_columns().len(), 177);
    assert_eq!(radius_256.classifier_columns().len(), 749);
    assert_eq!(radius_120.center, [-85, 102]);
    assert_eq!(radius_120.radius, 8);
    assert_eq!(
        radius_120.publisher_geometry,
        Some(super::PublisherViewGeometry {
            center_blocks: [-1350, 1634],
            radius_blocks: 120,
        })
    );
    assert!(
        !radius_120
            .classifier_columns()
            .contains(&ChunkKey::new(0, -77, 102))
    );
}

#[test]
fn transport_ack_after_disjoint_teleport_cannot_restore_purged_origin_work() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 1 }, 1),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let request = stream.pop_next_request().unwrap();
    stream.record_sub_chunk_request_transport_pending(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
    );
    assert_eq!(stream.transport_pending_requests, 1);

    stream
        .submit(
            2,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                mode: MovePlayerMode::Teleport,
                teleported: true,
                ..Default::default()
            }),
        )
        .unwrap();
    assert!(!stream.tracked_columns().contains(&request.chunk));
    assert!(stream.sub_chunk_deadlines.is_empty());

    stream.acknowledge_sub_chunk_request_sent(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
        Instant::now(),
    );

    assert_eq!(stream.transport_pending_requests, 0);
    assert!(stream.sub_chunk_deadlines.is_empty());
    assert!(!stream.tracked_columns().contains(&request.chunk));
    assert_eq!(stream.outstanding_sub_chunk_count(), 0);
}

#[test]
fn provisional_publisher_epoch_overflow_clears_retained_destination_membership() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.publisher_epoch = u64::MAX;
    stream.committed_view_cohort = Some(super::ViewCohort {
        dimension: 0,
        center: [0, 0],
        radius: 8,
        publisher_geometry: None,
    });
    stream.publisher_radius_chunks = Some(8);
    stream
        .submit(
            1,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                mode: MovePlayerMode::Teleport,
                teleported: true,
                ..Default::default()
            }),
        )
        .unwrap();
    let destination = ChunkKey::new(0, 65, 65);
    stream.required_columns.insert(destination);

    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 128,
            }),
        )
        .unwrap();

    assert_eq!(stream.publisher_epoch, u64::MAX);
    assert_eq!(stream.committed_view_cohort(), None);
    assert!(!stream.provisional_publisher_rebase);
    assert!(stream.required_columns.is_empty());
}

#[test]
fn provisional_publisher_update_retains_only_destination_work_in_clamped_active_scope() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.chunk_radius = Some(2);
    stream.publisher_radius_chunks = Some(2);
    stream.committed_view_cohort = Some(super::ViewCohort {
        dimension: 0,
        center: [0, 0],
        radius: 2,
        publisher_geometry: None,
    });
    stream
        .submit(
            1,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                mode: MovePlayerMode::Teleport,
                teleported: true,
                ..Default::default()
            }),
        )
        .unwrap();
    let active = ChunkKey::new(0, 66, 65);
    let publisher_only = ChunkKey::new(0, 69, 65);
    stream.required_columns = BTreeSet::from([active, publisher_only]);
    stream.loaded_columns = stream.required_columns.clone();

    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 128,
            }),
        )
        .unwrap();

    assert_eq!(stream.required_columns, BTreeSet::from([active]));
    // The publisher-clamped publication cohort keeps only the destination-scope
    // column, but chunk-grid retention is independent of the publisher: the
    // vanilla client's grid still retains both loaded columns because
    // publisher_only sits within the confirmed radius plus the grid slack of the
    // player's chunk.
    assert!(stream.tracked_columns().contains(&active));
    assert!(stream.tracked_columns().contains(&publisher_only));
}

#[test]
fn raw_128_epoch_requires_all_177_announced_columns() {
    const PUBLISHER_CENTER: [i32; 3] = [-1350, 104, 1634];
    const PUBLISHER_RADIUS_BLOCKS: u32 = 128;

    let target = super::ViewCohort::from_publisher(0, PUBLISHER_CENTER, PUBLISHER_RADIUS_BLOCKS);
    let required = target.classifier_columns();
    assert_eq!(required.len(), 177);

    let new_stream = || {
        WorldStream::new(WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        })
    };
    let announce_columns = |stream: &mut WorldStream, columns: &[ChunkKey]| {
        stream
            .submit(
                1,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center: PUBLISHER_CENTER,
                    radius_blocks: PUBLISHER_RADIUS_BLOCKS,
                }),
            )
            .unwrap();
        for (index, column) in columns.iter().enumerate() {
            stream
                .submit(
                    index as u64 + 2,
                    request_level_chunk_event(
                        column.dimension,
                        column.x,
                        column.z,
                        LevelChunkMode::LimitedRequests { highest: 0 },
                        1,
                    ),
                )
                .unwrap();
            if (index + 1) % super::MAX_ADMITTED_HEAVY_EVENTS == 0 {
                complete_pending_decode_jobs(stream);
            }
        }
        complete_pending_decode_jobs(stream);
    };

    let required_columns = required.iter().copied().collect::<Vec<_>>();
    let mut complete = new_stream();
    announce_columns(&mut complete, &required_columns);
    let complete_status = complete.cohort_status(target);
    assert_eq!(complete.committed_view_cohort(), Some(target));
    assert_eq!(complete_status.publisher_epoch, 1);
    assert_eq!(complete_status.expected, 177);
    assert_eq!(complete_status.loaded_target, 177);
    assert!(complete_status.is_exact());

    let missing = *required.last().expect("raw-radius cohort is non-empty");
    let mut incomplete = new_stream();
    announce_columns(&mut incomplete, &required_columns);
    incomplete.loaded_columns.remove(&missing);
    let incomplete_status = incomplete.cohort_status(target);
    assert_eq!(incomplete.loaded_column_count(), 176);
    assert_eq!(incomplete_status.expected, 177);
    assert_eq!(incomplete_status.loaded_target, 176);
    assert_eq!(incomplete_status.missing_target, 1);
    assert_eq!(incomplete_status.foreign_loaded, 0);
    assert!(!incomplete_status.is_exact());
}

#[test]
fn later_outer_announcement_expands_the_same_epoch_and_invalidates_old_identity() {
    const CENTER: [i32; 3] = [0, 64, 0];
    let target = super::ViewCohort::from_publisher(0, CENTER, 128);
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
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
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: CENTER,
                radius_blocks: 128,
            }),
        )
        .unwrap();
    for (index, column) in target.classifier_columns().iter().enumerate() {
        stream
            .submit(
                index as u64 + 2,
                request_level_chunk_event(
                    column.dimension,
                    column.x,
                    column.z,
                    LevelChunkMode::LimitedRequests { highest: 0 },
                    1,
                ),
            )
            .unwrap();
        if (index + 1) % super::MAX_ADMITTED_HEAVY_EVENTS == 0 {
            complete_pending_decode_jobs(&mut stream);
        }
    }
    complete_pending_decode_jobs(&mut stream);
    let before = stream.cohort_status(target);
    assert!(before.is_exact());

    let outer = ChunkKey::new(0, target.center[0] + target.radius, target.center[1]);
    assert!(!target.classifier_columns().contains(&outer));
    stream
        .submit(
            179,
            request_level_chunk_event(
                outer.dimension,
                outer.x,
                outer.z,
                LevelChunkMode::LimitedRequests { highest: 1 },
                1,
            ),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    let expanded = stream.cohort_status(target);
    assert_eq!(expanded.publisher_epoch, before.publisher_epoch);
    assert_eq!(expanded.expected, 178);
    assert_ne!(expanded.required_hash, before.required_hash);
    assert_eq!(expanded.loaded_target, 177);
    assert!(!expanded.is_exact());

    let requested = SubChunkKey::from_chunk(outer, -4);
    apply_sub_chunk_result(
        &mut stream,
        requested,
        super::PreparedSubChunkResult::AllAir,
    );
    assert!(stream.cohort_status(target).is_exact());
}

#[test]
fn publisher_identity_and_dimension_changes_reset_required_membership_epoch() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let publish = |stream: &mut WorldStream, sequence, center, radius_blocks| {
        stream
            .submit(
                sequence,
                WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                    center,
                    radius_blocks,
                }),
            )
            .unwrap();
    };

    publish(&mut stream, 1, [0, 64, 0], 128);
    stream
        .submit(
            2,
            request_level_chunk_event(0, 0, 0, LevelChunkMode::LimitedRequests { highest: 0 }, 1),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let first = stream.phase2_publication_snapshot(ChunkKey::new(0, 0, 0));
    assert_eq!(first.publisher_epoch, 1);
    assert_eq!(first.required_columns, 1);

    publish(&mut stream, 3, [0, 64, 0], 128);
    let repeated = stream.phase2_publication_snapshot(ChunkKey::new(0, 0, 0));
    assert_eq!(repeated.publisher_epoch, 1);
    assert_eq!(repeated.required_columns, 1);

    publish(&mut stream, 4, [16, 64, 0], 128);
    let moved = stream.phase2_publication_snapshot(ChunkKey::new(0, 1, 0));
    assert_eq!(moved.publisher_epoch, 2);
    assert_eq!(moved.required_columns, 0);

    stream
        .submit(
            5,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 80.0, 0.0],
            }),
        )
        .unwrap();
    publish(&mut stream, 6, [0, 80, 0], 128);
    let changed_dimension = stream.phase2_publication_snapshot(ChunkKey::new(1, 0, 0));
    assert_eq!(changed_dimension.publisher_epoch, 3);
    assert_eq!(changed_dimension.required_columns, 0);

    let other_session = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    assert_ne!(other_session.actor_session_id(), first.session_generation);
}

#[test]
fn required_epoch_reports_stable_only_after_stream_work_drains() {
    let target = super::ViewCohort::from_publisher(0, [0, 64, 0], 128);
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream.committed_view_cohort = Some(target);
    stream.publisher_epoch = 1;
    stream.required_columns.insert(ChunkKey::new(0, 0, 0));
    stream.loaded_columns.insert(ChunkKey::new(0, 0, 0));

    let drained = stream.phase2_publication_snapshot(ChunkKey::new(0, 0, 0));
    assert!(drained.required_cohort_stable);

    stream.transport_pending_requests = 1;
    let pending = stream.phase2_publication_snapshot(ChunkKey::new(0, 0, 0));
    assert!(!pending.required_cohort_stable);
}

#[test]
fn announcements_outside_clamped_retention_do_not_expand_required_epoch() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(16))
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 64, 0],
                radius_blocks: 272,
            }),
        )
        .unwrap();
    stream
        .submit(
            3,
            request_level_chunk_event(0, 17, 0, LevelChunkMode::LimitedRequests { highest: 0 }, 1),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream
            .phase2_publication_snapshot(ChunkKey::new(0, 0, 0))
            .required_columns,
        0
    );

    stream
        .submit(
            4,
            request_level_chunk_event(0, 16, 0, LevelChunkMode::LimitedRequests { highest: 0 }, 1),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream
            .phase2_publication_snapshot(ChunkKey::new(0, 0, 0))
            .required_columns,
        1
    );
}

#[test]
fn publisher_epoch_overflow_fails_closed_without_reusing_an_identity() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let old = super::ViewCohort::from_publisher(0, [0, 64, 0], 128);
    stream.publisher_epoch = u64::MAX;
    stream.committed_view_cohort = Some(old);
    stream.required_columns.insert(ChunkKey::new(0, 0, 0));

    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [16, 64, 0],
                radius_blocks: 128,
            }),
        )
        .unwrap();

    assert_eq!(stream.publisher_epoch, u64::MAX);
    assert_eq!(stream.committed_view_cohort(), None);
    assert!(stream.required_columns.is_empty());
}
