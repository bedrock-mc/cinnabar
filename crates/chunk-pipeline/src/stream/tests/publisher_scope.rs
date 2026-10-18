use super::*;

#[test]
fn publisher_required_disk_is_distinct_from_square_prefetch_scope() {
    let cohort = super::ViewCohort {
        dimension: 0,
        center: [10, -10],
        radius: 1,
        publisher_geometry: None,
    };

    assert_eq!(cohort.classifier_columns().len(), 5);
    assert!(
        cohort
            .classifier_columns()
            .contains(&ChunkKey::new(0, 11, -10))
    );
    assert!(
        !cohort
            .classifier_columns()
            .contains(&ChunkKey::new(0, 11, -9))
    );
    assert!(cohort.contains_column(0, [11, -9]));
    assert!(!cohort.contains_column(0, [12, -10]));
}

#[test]
fn publisher_block_position_and_radius_use_euclidean_chunk_conversion() {
    let cases = [
        ([-1, 64, -1], 0, [-1, -1], 0),
        ([-16, 64, -16], 1, [-1, -1], 1),
        ([-17, 64, -17], 15, [-2, -2], 1),
        ([15, 64, 15], 16, [0, 0], 1),
        ([16, 64, 16], 17, [1, 1], 2),
    ];

    for (center, radius_blocks, expected_center, expected_radius) in cases {
        let cohort = super::ViewCohort::from_publisher(3, center, radius_blocks);
        assert_eq!(cohort.dimension, 3);
        assert_eq!(cohort.center, expected_center);
        assert_eq!(cohort.radius, expected_radius);
    }
}

#[test]
fn unannounced_prefetch_column_prevents_explicit_cohort_exactness() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let target = super::ViewCohort::from_publisher(0, [0, 64, 0], 16);
    stream.publisher.cohort = Some(target);
    stream.publisher.epoch = 1;
    stream.publisher.required_columns = super::ViewCohort {
        publisher_geometry: None,
        ..target
    }
    .classifier_columns();
    stream.loaded_columns = stream.publisher.required_columns.clone();
    stream.loaded_columns.insert(ChunkKey::new(0, 1, 1));

    let status = stream.cohort_status(target);

    assert_eq!(status.expected, 5);
    assert_eq!(status.loaded_target, 5);
    assert_eq!(status.missing_target, 0);
    assert_eq!(status.foreign_loaded, 1);
    assert!(!status.is_exact());
}

#[test]
fn in_scope_prefetch_cannot_replace_a_missing_required_column() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let target = super::ViewCohort::from_publisher(0, [0, 64, 0], 16);
    stream.publisher.cohort = Some(target);
    stream.publisher.epoch = 1;
    stream.publisher.required_columns = super::ViewCohort {
        publisher_geometry: None,
        ..target
    }
    .classifier_columns();
    stream.loaded_columns = stream.publisher.required_columns.clone();
    stream.loaded_columns.remove(&ChunkKey::new(0, 1, 0));
    stream.loaded_columns.insert(ChunkKey::new(0, 1, 1));

    let status = stream.cohort_status(target);

    assert_eq!(stream.loaded_columns.len(), status.expected);
    assert_eq!(status.loaded_target, 4);
    assert_eq!(status.missing_target, 1);
    assert_eq!(status.foreign_loaded, 1);
    assert!(!status.is_exact());
}

#[test]
fn columns_outside_square_prefetch_scope_prevent_exact_cohort_readiness() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let target = super::ViewCohort {
        dimension: 0,
        center: [0, 0],
        radius: 1,
        publisher_geometry: None,
    };
    stream.publisher.cohort = Some(target);
    stream.loaded_columns = target.classifier_columns();
    stream.loaded_columns.insert(ChunkKey::new(0, 2, 0));

    let status = stream.cohort_status(target);

    assert_eq!(status.foreign_loaded, 1);
    assert!(status.target_is_complete());
    assert!(!status.is_exact());
}

#[test]
fn publisher_cohort_is_exposed_only_after_fifo_commit() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let target = super::ViewCohort {
        dimension: 0,
        center: [100, 0],
        radius: 16,
        publisher_geometry: Some(client_world::PublisherViewGeometry {
            center_blocks: [1_600, 0],
            radius_blocks: 256,
        }),
    };

    stream.submit(1, inline_air_event(0)).unwrap();
    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_600, 64, 0],
                radius_blocks: 256,
            }),
        )
        .unwrap();

    assert_ne!(stream.cohort_status(target).committed, Some(target));

    complete_pending_decode_jobs(&mut stream);

    assert_eq!(stream.cohort_status(target).committed, Some(target));
}

#[test]
fn publisher_cohort_accessor_is_exposed_only_after_fifo_commit() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let target = super::ViewCohort {
        dimension: 0,
        center: [100, 0],
        radius: 16,
        publisher_geometry: Some(client_world::PublisherViewGeometry {
            center_blocks: [1_600, 0],
            radius_blocks: 256,
        }),
    };

    assert_eq!(stream.committed_view_cohort(), None);

    stream
        .submit(
            2,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_600, 64, 0],
                radius_blocks: 256,
            }),
        )
        .unwrap();

    assert_eq!(stream.committed_view_cohort(), None);

    stream
        .submit(1, WorldEvent::ChunkRadiusUpdated(16))
        .unwrap();

    assert_eq!(stream.committed_view_cohort(), Some(target));
}

#[test]
fn source_capture_occurs_at_move_fifo_commit_before_later_publisher_eviction() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let source = ChunkKey::new(0, 0, 0);
    let source_cohort = super::ViewCohort {
        dimension: 0,
        center: [0, 0],
        radius: 16,
        publisher_geometry: Some(client_world::PublisherViewGeometry {
            center_blocks: [0, 0],
            radius_blocks: 256,
        }),
    };
    stream.loaded_columns.insert(source);
    stream.chunk_radius = Some(16);
    stream.schedule_source_capture(2);

    stream
        .submit(
            2,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [1_040.5, 70.0, 1_040.5],
                pitch: 0.0,
                yaw: 0.0,
                ..Default::default()
            }),
        )
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [1_040, 70, 1_040],
                radius_blocks: 256,
            }),
        )
        .unwrap();
    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 70, 0],
                radius_blocks: 256,
            }),
        )
        .unwrap();

    assert!(stream.publisher.source_columns.contains(&source));
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [super::CommittedControlEvent::MovePlayer {
            sequence: 2,
            source_cohort: Some(cohort),
            ..
        }] if *cohort == source_cohort
    ));
    assert!(stream.retain_local([1_040.5, 70.0, 1_040.5]));
    assert!(!stream.tracked_columns().contains(&source));
    assert!(stream.publisher.source_columns.contains(&source));
}

#[test]
fn only_disjoint_local_teleports_may_provisionally_rebase_publisher_retention() {
    fn stream_at_origin() -> WorldStream {
        let mut stream = WorldStream::new(WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.5, 70.0, 0.5],
            world_spawn_position: [0, 70, 0],
            air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
            block_network_ids_are_hashes: false,
        });
        stream.chunk_radius = Some(8);
        stream.publisher.radius_chunks = Some(8);
        let resident = SubChunkKey::new(0, 0, -4, 0);
        let requested = SubChunkKey::new(0, 1, -4, 0);
        let deadline = Instant::now() + super::SUB_CHUNK_RESPONSE_TIMEOUT;
        let pending = super::PendingSubChunk {
            response_deadline: Some(deadline),
            ..Default::default()
        };
        stream.loaded_columns.insert(resident.chunk());
        stream.resident.insert(resident);
        stream
            .requests
            .requested
            .insert(requested.chunk(), BTreeMap::from([(requested.y, pending)]));
        stream.requests.deadlines.insert((deadline, requested));
        stream
    }

    for movement in [
        MovePlayerEvent {
            runtime_id: 2,
            position: [1_040.5, 70.0, 1_040.5],
            mode: MovePlayerMode::Teleport,
            teleported: true,
            ..Default::default()
        },
        MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, 70.0, 1_040.5],
            mode: MovePlayerMode::Normal,
            ..Default::default()
        },
        MovePlayerEvent {
            runtime_id: 1,
            position: [16.5, 70.0, 16.5],
            mode: MovePlayerMode::Teleport,
            teleported: true,
            ..Default::default()
        },
    ] {
        let mut stream = stream_at_origin();
        stream.submit(1, WorldEvent::MovePlayer(movement)).unwrap();
        // None of these moves is a disjoint local teleport, so none provisionally
        // rebases the publisher: the publisher center, armed local resets, and
        // rebase flag are all left untouched. Chunk-grid retention recenters on
        // the local player independently and is covered by its own tests.
        assert_eq!(stream.publisher.center, Some([0, 70, 0]));
        assert_eq!(stream.publisher.local_reset.armed, 0);
        assert!(!stream.publisher.provisional_rebase);
    }
}

#[test]
fn publisher_cohort_preserves_over_max_radius_while_runtime_scope_clamps() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let max = super::MAX_VIEW_RADIUS_CHUNKS;
    let over_blocks = (max as u32 + 1) * 16;
    let target = super::ViewCohort {
        dimension: 0,
        center: [0, 0],
        radius: max,
        publisher_geometry: None,
    };

    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 64, 0],
                radius_blocks: over_blocks,
            }),
        )
        .unwrap();

    assert_eq!(
        stream.cohort_status(target).committed,
        Some(super::ViewCohort {
            dimension: 0,
            center: [0, 0],
            radius: max + 1,
            publisher_geometry: Some(client_world::PublisherViewGeometry {
                center_blocks: [0, 0],
                radius_blocks: over_blocks,
            }),
        })
    );
    assert_eq!(stream.stats().publisher_radius_chunks, Some(max));
}

#[test]
fn equal_resident_and_known_air_counts_with_key_replacement_change_identity_hashes() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let target = super::ViewCohort {
        dimension: 0,
        center: [0, 0],
        radius: 1,
        publisher_geometry: None,
    };
    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(PublisherUpdateEvent {
                center: [0, 64, 0],
                radius_blocks: 16,
            }),
        )
        .unwrap();
    let key_a = SubChunkKey::new(0, -1, 0, -1);
    let key_b = SubChunkKey::new(0, 1, 0, 1);
    stream.record_known_air(key_a);
    let before = stream.cohort_status(target);

    stream.resident.clear();
    stream.known_air.clear();
    stream.record_known_air(key_b);
    let after = stream.cohort_status(target);

    assert_eq!(before.resident_count, after.resident_count);
    assert_ne!(before.resident_hash, after.resident_hash);
    assert_eq!(before.known_air_count, after.known_air_count);
    assert_ne!(before.known_air_hash, after.known_air_hash);
}
