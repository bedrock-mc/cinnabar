use super::*;
#[test]
fn transparent_sort_marker_requires_new_presented_committed_generation_with_refs() {
    let valid = TransparentSortMetricsSnapshot {
        committed_generation: 17,
        presented_generation: 17,
        ref_count: 99,
        ..Default::default()
    };
    assert_eq!(
        transparent_sort_committed_marker(16, valid),
        Some(format!(
            "{TRANSPARENT_SORT_COMMITTED} generation=17 ref_count=99"
        ))
    );
    assert_eq!(transparent_sort_committed_marker(17, valid), None);
    assert_eq!(
        transparent_sort_committed_marker(
            16,
            TransparentSortMetricsSnapshot {
                presented_generation: 17,
                committed_generation: 18,
                ref_count: 99,
                ..Default::default()
            }
        ),
        None
    );
    assert_eq!(
        transparent_sort_committed_marker(
            16,
            TransparentSortMetricsSnapshot {
                presented_generation: 17,
                committed_generation: 17,
                ref_count: 0,
                ..Default::default()
            }
        ),
        None
    );
}

#[test]
fn transparent_sort_marker_writer_targets_and_flushes_stdout_sink() {
    struct FlushRecordingWriter {
        bytes: Vec<u8>,
        flushed: bool,
    }

    impl std::io::Write for FlushRecordingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushed = true;
            Ok(())
        }
    }

    let mut writer = FlushRecordingWriter {
        bytes: Vec::new(),
        flushed: false,
    };
    write_stdout_marker(
        &mut writer,
        &format!("{TRANSPARENT_SORT_COMMITTED} generation=17 ref_count=99"),
    );

    assert_eq!(
        writer.bytes,
        format!("{TRANSPARENT_SORT_COMMITTED} generation=17 ref_count=99\n").as_bytes()
    );
    assert!(writer.flushed);
}

#[test]
fn exact_full_view_proof_uses_stable_presented_transparent_sort_generation() {
    let completion = binding_teleport_completion(Instant::now(), Duration::from_millis(1_500));

    let proof = teleport_proof(exact_destination_status(), &completion);

    assert_eq!(proof.exact.transparent_sort_generation, 17);
    assert!(
        exact_full_view_proof_marker_fields(&proof.exact)
            .contains("transparent_sort_generation=17")
    );
}

#[test]
fn full_view_teleport_arms_only_with_a_fifo_committed_source_cohort() {
    let started = Instant::now();
    let movement = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
        runtime_id: 1,
        position: [1_040.5, 70.0, 1_040.5],
        pitch: 0.0,
        yaw: 0.0,
        ..Default::default()
    });
    let mut tracker = FullViewTeleportTracker::new(true);
    tracker.set_source_mutation_coordinate([0, 58, 0]);
    tracker.begin_world_ready([0.5, 70.0, 0.5], 1);

    assert!(tracker.observe_ingress(&movement, 1, started, 0, 10));
    let WorldEvent::MovePlayer(move_player) = &movement else {
        unreachable!();
    };
    let move_player = *move_player;
    assert!(!tracker.commit_move(1, move_player, None));
    assert!(tracker.pending.is_none());
    assert!(tracker.observe_ingress(&movement, 2, started + Duration::from_millis(1), 0, 11,));
    assert!(tracker.commit_move(2, move_player, Some(SOURCE_COHORT)));
    assert_eq!(
        tracker.pending.as_ref().map(|pending| pending.source),
        Some(SOURCE_COHORT)
    );
}

#[test]
fn full_view_teleport_preserves_the_servers_authoritative_source_radius() {
    let started = Instant::now();
    let movement = protocol::MovePlayerEvent {
        runtime_id: 1,
        position: [1_040.5, 70.0, 1_040.5],
        pitch: 0.0,
        yaw: 0.0,
        ..Default::default()
    };
    let source = ViewCohort {
        radius: 8,
        ..SOURCE_COHORT
    };
    let mut tracker = FullViewTeleportTracker::new(true);
    tracker.set_source_mutation_coordinate([0, 58, 0]);
    tracker.begin_world_ready([0.5, 70.0, 0.5], 1);

    assert!(tracker.observe_ingress(&WorldEvent::MovePlayer(movement), 1, started, 0, 10,));
    assert!(tracker.commit_move(1, movement, Some(source)));
    let pending = tracker.pending.as_ref().expect("teleport should arm");
    assert_eq!(pending.source.radius, 8);
    assert_eq!(pending.target.radius, 8);
}

#[test]
fn movement_correction_never_arms_full_view_teleport_tracking() {
    let started = Instant::now();
    let correction = protocol::PlayerMovementCorrectionEvent {
        position: [1_040.5, 70.0, 1_040.5],
        delta: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        on_ground: true,
        tick: 55,
    };
    let mut tracker = FullViewTeleportTracker::new(true);
    tracker.set_source_mutation_coordinate([0, 58, 0]);
    tracker.begin_world_ready([0.5, 70.0, 0.5], 1);

    assert!(!tracker.observe_ingress(
        &WorldEvent::PlayerMovementCorrection(correction),
        1,
        started,
        0,
        10,
    ));
    assert!(
        !tracker.observe_committed_control(&CommittedControlEvent::PlayerMovementCorrection {
            sequence: 1,
            correction,
            resolved: client_world::ResolvedServerPosition {
                position: correction.position,
                surface_anchor: None,
            },
        })
    );
    assert!(tracker.pending.is_none());
}

#[test]
fn out_of_order_move_waits_for_fifo_source_commit_before_arming() {
    let started = Instant::now();
    let movement = protocol::MovePlayerEvent {
        runtime_id: 1,
        position: [1_040.5, 70.0, 1_040.5],
        pitch: 0.0,
        yaw: 0.0,
        ..Default::default()
    };
    let mut tracker = FullViewTeleportTracker::new(true);
    tracker.set_source_mutation_coordinate([0, 58, 0]);
    tracker.begin_world_ready([0.5, 70.0, 0.5], 1);
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.5, 70.0, 0.5],
        world_spawn_position: [0, 70, 0],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });

    assert!(tracker.observe_ingress(&WorldEvent::MovePlayer(movement), 2, started, 0, 10,));
    stream.schedule_source_capture(2);
    stream.submit(2, WorldEvent::MovePlayer(movement)).unwrap();
    assert!(stream.take_committed_controls().is_empty());
    assert!(tracker.pending.is_none());

    stream
        .submit(
            1,
            WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
                center: [0, 70, 0],
                radius_blocks: 256,
            }),
        )
        .unwrap();
    let controls = stream.take_committed_controls();
    assert_eq!(controls.len(), 1);
    assert!(tracker.observe_committed_control(&controls[0]));
    let pending = tracker.pending.as_ref().unwrap();
    assert_eq!(pending.started, started);
    assert_eq!(pending.source, SOURCE_COHORT);
    assert_eq!(stream.committed_view_cohort(), Some(SOURCE_COHORT));
}

#[test]
fn write_buffer_ack_alone_never_settles_binding_teleport() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let proposal = proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]);

    let first = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposal.clone(),
            started + Duration::from_millis(200),
        )
        .expect("an exact clean cohort should freeze a render expectation");
    let unchanged = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposal,
            started + Duration::from_secs(30),
        )
        .expect("an unchanged exact cohort should retain its expectation");

    assert_eq!(unchanged, first);
    assert_eq!(tracker.completed, None);
    assert!(tracker.pending.is_some());
}

#[test]
fn non_empty_leaf_forest_never_binds_an_empty_target_expectation() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let empty = proposed_render_expectation(started + Duration::from_millis(200), []);

    assert_eq!(
        tracker.reconcile_presented_expectation(
            settled_teleport_snapshot(),
            empty,
            started + Duration::from_millis(200),
        ),
        None
    );
    assert!(tracker.pending.is_some());
    assert_eq!(tracker.completed, None);
    assert_eq!(tracker.completed_target_mutation, None);
}

#[test]
fn binding_teleport_requires_two_identical_presented_gpu_completed_frames() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let expectation = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
            started + Duration::from_millis(200),
        )
        .unwrap();
    let first = presented_acknowledgement(
        &expectation,
        10,
        Duration::from_millis(10),
        Duration::from_millis(20),
    );
    let second = presented_acknowledgement(
        &expectation,
        11,
        Duration::from_millis(30),
        Duration::from_millis(40),
    );

    assert_eq!(tracker.observe_presented_frame(first), None);
    let completion = tracker
        .observe_presented_frame(second)
        .expect("the adjacent second exact GPU-complete frame should settle");
    assert_eq!(completion.settle_latency, Duration::from_millis(240));
    assert_eq!(tracker.completed, Some(Duration::from_millis(240)));
}

#[test]
fn render_manifest_change_resets_teleport_stability() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let key_a = SubChunkKey::new(0, 64, 65, 65);
    let key_b = SubChunkKey::new(0, 65, 65, 65);
    let expectation_a = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposed_render_expectation(started + Duration::from_millis(200), [(key_a, 7)]),
            started + Duration::from_millis(200),
        )
        .unwrap();
    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &expectation_a,
            1,
            Duration::from_millis(10),
            Duration::from_millis(20),
        )),
        None
    );

    let expectation_b = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposed_render_expectation(
                started + Duration::from_millis(300),
                [(key_a, 7), (key_b, 8)],
            ),
            started + Duration::from_millis(300),
        )
        .unwrap();
    assert_ne!(expectation_b.view_generation, expectation_a.view_generation);
    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &expectation_b,
            2,
            Duration::from_millis(10),
            Duration::from_millis(20),
        )),
        None,
        "the first frame for a replacement manifest must only re-arm stability"
    );
    assert!(
        tracker
            .observe_presented_frame(presented_acknowledgement(
                &expectation_b,
                3,
                Duration::from_millis(30),
                Duration::from_millis(40),
            ))
            .is_some()
    );
}

#[test]
fn source_render_instance_blocks_settle_with_clean_world_queues() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let expectation = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
            started + Duration::from_millis(200),
        )
        .unwrap();
    for sequence in [1, 2] {
        let mut blocked = presented_acknowledgement(
            &expectation,
            sequence,
            Duration::from_millis(sequence * 10),
            Duration::from_millis(sequence * 10 + 5),
        );
        blocked.source_instances = 1;
        assert_eq!(tracker.observe_presented_frame(blocked), None);
    }
    assert_eq!(tracker.completed, None);
    assert!(tracker.pending.is_some());
}

#[test]
fn cohort_identity_change_resets_stability_even_when_counts_match() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let first_expectation = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
            started + Duration::from_millis(200),
        )
        .unwrap();
    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &first_expectation,
            1,
            Duration::from_millis(10),
            Duration::from_millis(20),
        )),
        None
    );

    let mut changed_snapshot = settled_teleport_snapshot();
    changed_snapshot.cohort.as_mut().unwrap().resident_hash ^= 0x55aa;
    let replacement = tracker
        .reconcile_presented_expectation(
            changed_snapshot,
            proposed_render_expectation(started + Duration::from_millis(300), [(key, 7)]),
            started + Duration::from_millis(300),
        )
        .unwrap();
    assert_ne!(
        replacement.view_generation,
        first_expectation.view_generation
    );
    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &first_expectation,
            2,
            Duration::from_millis(30),
            Duration::from_millis(40),
        )),
        None,
        "an acknowledgement for the old cohort identity must not settle"
    );
}

#[test]
fn visibility_count_change_does_not_reset_binding_render_stability() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let proposal = proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]);
    let expectation = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposal.clone(),
            started + Duration::from_millis(200),
        )
        .unwrap();
    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &expectation,
            1,
            Duration::from_millis(10),
            Duration::from_millis(20),
        )),
        None
    );

    let mut culled = settled_teleport_snapshot();
    culled.visible_sub_chunks = 0;
    let retained = tracker.reconcile_presented_expectation(
        culled,
        proposal,
        started + Duration::from_millis(220),
    );
    assert_eq!(
        retained,
        Some(expectation.clone()),
        "a non-binding culling count replaced the frozen expectation"
    );
    assert!(
        tracker
            .observe_presented_frame(presented_acknowledgement(
                &expectation,
                2,
                Duration::from_millis(30),
                Duration::from_millis(40),
            ))
            .is_some(),
        "a non-binding visibility change discarded the first exact frame"
    );
}

#[test]
fn teleport_stage_offsets_are_monotonic_and_settle_equals_stable_gpu_completion() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let expectation = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
            started + Duration::from_millis(200),
        )
        .unwrap();
    let mut malformed =
        presented_acknowledgement(&expectation, 9, Duration::ZERO, Duration::from_millis(1));
    malformed.present_returned_at = expectation.render_ready_at - Duration::from_millis(1);
    assert_eq!(tracker.observe_presented_frame(malformed), None);

    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &expectation,
            10,
            Duration::from_millis(10),
            Duration::from_millis(20),
        )),
        None
    );
    let completion = tracker
        .observe_presented_frame(presented_acknowledgement(
            &expectation,
            11,
            Duration::from_millis(30),
            Duration::from_millis(40),
        ))
        .unwrap();

    assert_eq!(completion.render_ready_latency, Duration::from_millis(200));
    assert_eq!(
        completion.first_present_return_latency,
        Duration::from_millis(210)
    );
    assert_eq!(
        completion.first_gpu_completion_latency,
        Duration::from_millis(220)
    );
    assert_eq!(
        completion.stable_present_return_latency,
        Duration::from_millis(230)
    );
    assert_eq!(
        completion.stable_gpu_completion_latency,
        Duration::from_millis(240)
    );
    assert_eq!(
        completion.settle_latency,
        completion.stable_gpu_completion_latency
    );
    assert_eq!(completion.view_generation, expectation.view_generation);
}

#[test]
fn presented_frame_timestamp_regression_resets_the_stability_pair() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let expectation = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
            started + Duration::from_millis(200),
        )
        .unwrap();
    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &expectation,
            1,
            Duration::from_millis(30),
            Duration::from_millis(40),
        )),
        None
    );
    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &expectation,
            2,
            Duration::from_millis(20),
            Duration::from_millis(50),
        )),
        None,
        "an earlier present-return timestamp formed a false stable pair"
    );
    let completion = tracker
        .observe_presented_frame(presented_acknowledgement(
            &expectation,
            3,
            Duration::from_millis(60),
            Duration::from_millis(70),
        ))
        .expect("a later adjacent monotonic frame should complete after the reset");
    assert_eq!(
        completion.first_present_return_latency,
        Duration::from_millis(220)
    );
    assert_eq!(completion.settle_latency, Duration::from_millis(270));
}

#[test]
fn radius_16_with_one_loaded_target_column_and_empty_work_never_arms() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let mut snapshot = settled_teleport_snapshot();
    let mut status = exact_destination_status();
    status.loaded_target = 1;
    status.missing_target = 796;
    status.resident_count = 1;
    status.known_air_count = 0;
    snapshot.loaded_columns = 1;
    snapshot.cohort = Some(status);

    assert_eq!(
        tracker.observe_snapshot(snapshot, started + Duration::from_millis(200)),
        None
    );
    assert_eq!(
        tracker.observe_snapshot(snapshot, started + Duration::from_secs(5)),
        None
    );
    assert!(!tracker.has_clean_candidate());
}

#[test]
fn equal_total_loaded_count_with_missing_target_and_foreign_source_never_arms() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let mut snapshot = settled_teleport_snapshot();
    let mut status = exact_destination_status();
    status.loaded_target = 796;
    status.missing_target = 1;
    status.foreign_loaded = 1;
    status.source_leftover = 1;
    snapshot.loaded_columns = 797;
    snapshot.cohort = Some(status);

    assert_eq!(
        tracker.observe_snapshot(snapshot, started + Duration::from_millis(200)),
        None
    );
    assert_eq!(
        tracker.observe_snapshot(snapshot, started + Duration::from_secs(5)),
        None
    );
    assert!(!tracker.has_clean_candidate());
}

#[test]
fn replacing_resident_key_at_equal_counts_resets_stability() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let first = settled_teleport_snapshot();
    assert_eq!(
        tracker.observe_snapshot(first, started + Duration::from_millis(200)),
        None
    );

    let mut replacement = first;
    replacement.cohort.as_mut().unwrap().resident_hash = 0x9999;
    assert_eq!(
        tracker.observe_snapshot(replacement, started + Duration::from_millis(2_300)),
        None,
        "equal resident counts with different keys retained the old stability candidate"
    );
    assert_eq!(
        tracker.observe_snapshot(replacement, started + Duration::from_millis(4_300)),
        None,
        "clean/upload state alone must never complete the presented-frame gate"
    );
    assert_eq!(tracker.completed, None);
}

#[test]
fn wrong_target_center_dimension_or_radius_never_arms() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    for (index, committed) in [
        ViewCohort {
            center: [64, 65],
            ..DESTINATION_COHORT
        },
        ViewCohort {
            dimension: 1,
            ..DESTINATION_COHORT
        },
        ViewCohort {
            radius: 15,
            ..DESTINATION_COHORT
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut snapshot = settled_teleport_snapshot();
        snapshot.cohort.as_mut().unwrap().committed = Some(committed);
        assert_eq!(
            tracker.observe_snapshot(
                snapshot,
                started + Duration::from_secs(u64::try_from(index).unwrap() * 3 + 1),
            ),
            None
        );
        assert!(!tracker.has_clean_candidate());
    }
}

#[test]
fn previously_seen_wrong_radius_publisher_does_not_arm_target_stage() {
    let started = Instant::now();
    let mut tracker = FullViewTeleportTracker::new(true);
    tracker.set_source_mutation_coordinate([0, 58, 0]);
    tracker.begin_world_ready([0.5, 70.0, 0.5], 1);
    tracker.observe(
        &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
            center: [1_040, 70, 1_040],
            radius_blocks: 240,
        }),
        started,
        0,
    );
    assert!(tracker.observe(
        &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, 70.0, 1_040.5],
            pitch: 0.0,
            yaw: 0.0,
            ..Default::default()
        }),
        started + Duration::from_millis(100),
        0,
    ));

    assert_eq!(
        tracker.observe_snapshot(
            settled_teleport_snapshot(),
            started + Duration::from_millis(200),
        ),
        None
    );
    assert_eq!(
        tracker.observe_snapshot(
            settled_teleport_snapshot(),
            started + Duration::from_millis(2_200),
        ),
        None
    );
    assert!(!tracker.has_clean_candidate());
}

#[test]
fn teleport_cohort_progress_is_target_tagged_formatted_and_rate_limited() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    let mut status = exact_destination_status();
    status.loaded_target = 1;
    status.missing_target = 796;
    let work = WorldReadyWork {
        outstanding_sub_chunks: 7,
        unacknowledged_meshes: 3,
        ..Default::default()
    };
    let timeout_progress = SubChunkTimeoutProgress {
        awaiting_responses: 5,
        timeouts: 4,
        retries_scheduled: 3,
        retry_exhaustions: 2,
    };

    let line = tracker
        .cohort_progress_line(
            status,
            work,
            timeout_progress,
            started + Duration::from_millis(200),
        )
        .expect("first pending cohort observation should be inspectable");
    assert!(line.starts_with(&format!("{TELEPORT_COHORT} target=0:65:65:16")));
    assert!(line.contains("committed=0:65:65:16"));
    assert!(line.contains("expected=797 loaded_target=1 missing_target=796"));
    assert!(line.contains("resident_count=9000 resident_hash=0000000000001234"));
    assert!(line.contains("known_air_count=1000 known_air_hash=0000000000005678"));
    assert!(line.contains("outstanding_sub_chunks=7"));
    assert!(line.contains("awaiting_sub_chunk_responses=5"));
    assert!(line.contains("sub_chunk_timeouts=4"));
    assert!(line.contains("sub_chunk_retries_scheduled=3"));
    assert!(line.contains("sub_chunk_retry_exhaustions=2"));
    assert!(line.contains("unacknowledged_meshes=3"));
    assert_eq!(
        tracker.cohort_progress_line(
            status,
            work,
            timeout_progress,
            started + Duration::from_millis(1_199),
        ),
        None
    );
    assert!(
        tracker
            .cohort_progress_line(
                status,
                work,
                timeout_progress,
                started + Duration::from_millis(1_200),
            )
            .is_some()
    );
}

#[test]
fn global_stream_timestamps_are_emitted_as_separate_target_tagged_diagnostics() {
    let mut completion = binding_teleport_completion(Instant::now(), Duration::from_millis(1_500));
    completion.last_chunk_commit_latency = Some(Duration::from_millis(10));
    completion.last_mesh_dispatch_latency = Some(Duration::from_millis(20));
    completion.last_mesh_completion_latency = Some(Duration::from_millis(30));
    completion.last_mesh_ack_latency = Some(Duration::from_millis(40));
    let marker = teleport_global_stage_diagnostic_marker(DESTINATION_COHORT, &completion);

    assert_eq!(
        marker,
        format!(
            "{TELEPORT_GLOBAL_STAGE_DIAGNOSTIC} target=0:65:65:16 global_commit_ms=10.0000 global_mesh_dispatch_ms=20.0000 global_mesh_complete_ms=30.0000 global_mesh_ack_ms=40.0000"
        )
    );
}

#[test]
fn foreign_and_source_events_do_not_advance_target_stage_diagnostics() {
    let started = Instant::now();
    let mut tracker = destination_tracker(started);
    for (event, observed_at) in [
        (
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitlessRequests,
                payload: Vec::new(),
            }),
            started + Duration::from_millis(200),
        ),
        (
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 1,
                x: 65,
                z: 65,
                mode: LevelChunkMode::LimitlessRequests,
                payload: Vec::new(),
            }),
            started + Duration::from_millis(300),
        ),
        (
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    position: [0, -4, 0],
                    result: SubChunkResult::AllAir,
                }],
            }),
            started + Duration::from_millis(400),
        ),
        (
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 1,
                entries: vec![SubChunkEntryEvent {
                    position: [65, -4, 65],
                    result: SubChunkResult::AllAir,
                }],
            }),
            started + Duration::from_millis(500),
        ),
    ] {
        tracker.observe(&event, observed_at, 0);
    }
    tracker.observe(
        &WorldEvent::LevelChunk(LevelChunkEvent {
            dimension: 0,
            x: 65,
            z: 65,
            mode: LevelChunkMode::LimitlessRequests,
            payload: Vec::new(),
        }),
        started + Duration::from_millis(600),
        0,
    );
    tracker.observe(
        &WorldEvent::SubChunks(SubChunkBatchEvent {
            dimension: 0,
            entries: vec![
                SubChunkEntryEvent {
                    position: [0, -4, 0],
                    result: SubChunkResult::AllAir,
                },
                SubChunkEntryEvent {
                    position: [65, -4, 65],
                    result: SubChunkResult::AllAir,
                },
            ],
        }),
        started + Duration::from_millis(700),
        0,
    );
    assert_eq!(
        tracker.observe_snapshot(
            settled_teleport_snapshot(),
            started + Duration::from_millis(800),
        ),
        None
    );
    let expectation = tracker
        .pending
        .as_ref()
        .and_then(|pending| pending.presented_candidate.as_ref())
        .map(|candidate| candidate.expectation.clone())
        .unwrap();
    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &expectation,
            1,
            Duration::from_millis(10),
            Duration::from_millis(20),
        )),
        None
    );
    let completion = tracker
        .observe_presented_frame(presented_acknowledgement(
            &expectation,
            2,
            Duration::from_millis(30),
            Duration::from_millis(40),
        ))
        .unwrap();

    assert_eq!(completion.level_chunk_events, 1);
    assert_eq!(
        completion.first_level_chunk_latency,
        Some(Duration::from_millis(600))
    );
    assert_eq!(
        completion.last_level_chunk_latency,
        Some(Duration::from_millis(600))
    );
    assert_eq!(completion.sub_chunk_events, 1);
    assert_eq!(
        completion.first_sub_chunk_latency,
        Some(Duration::from_millis(700))
    );
    assert_eq!(
        completion.last_sub_chunk_latency,
        Some(Duration::from_millis(700))
    );
}

#[test]
fn full_view_teleport_requires_far_motion_matching_publisher_and_two_presented_frames() {
    let started = Instant::now();
    let mut tracker = FullViewTeleportTracker::new(true);
    tracker.set_source_mutation_coordinate([0, 58, 0]);
    tracker.begin_world_ready([0.5, 70.0, 0.5], 1);

    tracker.observe(
        &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [32.5, 70.0, 0.5],
            pitch: 0.0,
            yaw: 0.0,
            ..Default::default()
        }),
        started,
        0,
    );
    assert!(
        !tracker.is_pending(),
        "near movement armed a full-view gate"
    );

    tracker.observe(
        &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, 70.0, 1_040.5],
            pitch: 0.0,
            yaw: 0.0,
            ..Default::default()
        }),
        started,
        0,
    );
    assert!(tracker.is_pending());
    assert_eq!(
        tracker.observe_snapshot(
            settled_teleport_snapshot(),
            started + Duration::from_secs(1)
        ),
        None,
        "clean work settled before the matching publisher update"
    );

    tracker.observe(
        &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
            center: [1_040, 70, 1_040],
            radius_blocks: 256,
        }),
        started + Duration::from_millis(1_100),
        0,
    );
    assert_eq!(
        tracker.observe_snapshot(
            settled_teleport_snapshot(),
            started + Duration::from_millis(1_200),
        ),
        None
    );
    tracker.observe(
        &WorldEvent::LevelChunk(LevelChunkEvent {
            dimension: 0,
            x: 65,
            z: 65,
            mode: LevelChunkMode::LimitlessRequests,
            payload: Vec::new(),
        }),
        started + Duration::from_millis(1_150),
        0,
    );
    tracker.observe(
        &WorldEvent::SubChunks(protocol::SubChunkBatchEvent {
            dimension: 0,
            entries: vec![SubChunkEntryEvent {
                position: [65, -4, 65],
                result: SubChunkResult::AllAir,
            }],
        }),
        started + Duration::from_millis(1_300),
        0,
    );
    tracker.observe(
        &WorldEvent::SubChunks(protocol::SubChunkBatchEvent {
            dimension: 0,
            entries: vec![SubChunkEntryEvent {
                position: [65, -4, 65],
                result: SubChunkResult::AllAir,
            }],
        }),
        started + Duration::from_millis(1_500),
        0,
    );
    tracker.observe(
        &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
            center: [1_040, 70, 1_040],
            radius_blocks: 256,
        }),
        started + Duration::from_millis(1_600),
        0,
    );
    let mut clean = settled_teleport_snapshot();
    clean.last_chunk_commit_at = Some(started + Duration::from_millis(1_650));
    clean.last_mesh_dispatch_at = Some(started + Duration::from_millis(1_700));
    clean.last_mesh_completion_at = Some(started + Duration::from_millis(1_800));
    clean.last_mesh_ack_at = Some(started + Duration::from_millis(1_900));
    let mut busy = clean;
    busy.work.network_events = 4;
    busy.work.pending_mesh_jobs = 1;
    assert_eq!(
        tracker.observe_snapshot(busy, started + Duration::from_secs(2)),
        None,
        "late work did not reset the clean candidate"
    );
    assert_eq!(
        tracker.observe_snapshot(clean, started + Duration::from_millis(2_100),),
        None
    );
    let expectation = tracker
        .pending
        .as_ref()
        .and_then(|pending| pending.presented_candidate.as_ref())
        .map(|candidate| candidate.expectation.clone())
        .expect("clean exact target should freeze a render expectation");
    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &expectation,
            10,
            Duration::from_millis(10),
            Duration::from_millis(20),
        )),
        None
    );
    let completion = tracker
        .observe_presented_frame(presented_acknowledgement(
            &expectation,
            11,
            Duration::from_millis(30),
            Duration::from_millis(40),
        ))
        .expect("the adjacent second exact GPU-complete frame should complete");
    assert_eq!(completion.settle_latency, Duration::from_millis(2_140));
    assert_eq!(
        completion.publisher_latency,
        Some(Duration::from_millis(1_100))
    );
    assert_eq!(
        completion.first_level_chunk_latency,
        Some(Duration::from_millis(1_150))
    );
    assert_eq!(
        completion.last_level_chunk_latency,
        Some(Duration::from_millis(1_150))
    );
    assert_eq!(completion.level_chunk_events, 1);
    assert_eq!(
        completion.first_sub_chunk_latency,
        Some(Duration::from_millis(1_300))
    );
    assert_eq!(
        completion.last_sub_chunk_latency,
        Some(Duration::from_millis(1_500))
    );
    assert_eq!(completion.sub_chunk_events, 2);
    assert_eq!(
        completion.last_chunk_commit_latency,
        Some(Duration::from_millis(1_650))
    );
    assert_eq!(
        completion.last_mesh_dispatch_latency,
        Some(Duration::from_millis(1_700))
    );
    assert_eq!(
        completion.last_mesh_completion_latency,
        Some(Duration::from_millis(1_800))
    );
    assert_eq!(
        completion.last_mesh_ack_latency,
        Some(Duration::from_millis(1_900))
    );
    assert_eq!(completion.peak_network_events, 4);
}

#[test]
fn partial_target_column_coverage_never_settles_the_teleport_stream() {
    let started = Instant::now();
    let mut tracker = FullViewTeleportTracker::new(true);
    tracker.set_source_mutation_coordinate([0, 58, 0]);
    tracker.begin_world_ready([0.5, 70.0, 0.5], 1);
    tracker.observe(
        &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, 70.0, 1_040.5],
            pitch: 0.0,
            yaw: 0.0,
            ..Default::default()
        }),
        started,
        0,
    );
    tracker.observe(
        &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
            center: [1_040, 70, 1_040],
            radius_blocks: 256,
        }),
        started + Duration::from_millis(100),
        0,
    );
    let mut partial = settled_teleport_snapshot();
    let status = partial.cohort.as_mut().unwrap();
    status.loaded_target = status.expected - 1;
    status.missing_target = status.expected - status.loaded_target;
    partial.loaded_columns = status.loaded_target;

    assert_eq!(
        tracker.observe_snapshot(partial, started + Duration::from_millis(200)),
        None
    );
    assert_eq!(
        tracker.observe_snapshot(partial, started + Duration::from_secs(5)),
        None,
        "a quiet partial target view passed the coverage gate"
    );
    assert!(tracker.is_pending());
}
