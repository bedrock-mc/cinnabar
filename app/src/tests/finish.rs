mod completion;

use super::*;
#[test]
fn forced_remesh_starts_only_after_binding_teleport_completion() {
    let teleport_started = Instant::now();
    let binding = binding_teleport_completion(teleport_started, Duration::from_millis(1_500));
    let started = teleport_started + Duration::from_millis(1_501);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let manifest = ForcedRemeshManifest {
        started_at: started,
        entries: Arc::from([(key, 8)]),
    };
    let mut tracker = FullViewRemeshTracker::default();

    assert!(!tracker.start(None, exact_destination_status(), manifest.clone(), 90,));
    assert!(tracker.start(Some(&binding), exact_destination_status(), manifest, 90,));
    assert!(tracker.is_pending());
}

#[test]
fn fast_forced_remesh_does_not_replace_or_fix_a_slow_binding_teleport() {
    let teleport_started = Instant::now();
    let binding = binding_teleport_completion(teleport_started, Duration::from_millis(2_400));
    let started = teleport_started + Duration::from_millis(2_401);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let manifest = ForcedRemeshManifest {
        started_at: started,
        entries: Arc::from([(key, 8)]),
    };
    let mut tracker = FullViewRemeshTracker::default();
    assert!(tracker.start(Some(&binding), exact_destination_status(), manifest, 145,));
    let expectation = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            ForcedRemeshManifestState::Complete,
            Some(proposed_render_expectation(
                started + Duration::from_millis(10),
                [(key, 8)],
            )),
            started + Duration::from_millis(10),
            146,
        )
        .unwrap();
    assert_eq!(
        tracker.observe_presented_frame(
            presented_acknowledgement(
                &expectation,
                43,
                Duration::from_millis(20),
                Duration::from_millis(40),
            ),
            148,
        ),
        None
    );
    let forced = tracker
        .observe_presented_frame(
            presented_acknowledgement(
                &expectation,
                44,
                Duration::from_millis(80),
                Duration::from_millis(90),
            ),
            151,
        )
        .unwrap();

    assert_eq!(binding.settle_latency, Duration::from_millis(2_400));
    assert_eq!(forced.settle_latency, Duration::from_millis(100));
    assert!(binding.settle_latency > Duration::from_secs(2));
    assert!(forced.settle_latency < Duration::from_secs(2));
}

#[test]
fn forced_remesh_busy_gap_resets_the_presented_pair() {
    let teleport_started = Instant::now();
    let binding = binding_teleport_completion(teleport_started, Duration::from_millis(1_500));
    let started = teleport_started + Duration::from_millis(1_501);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let manifest = ForcedRemeshManifest {
        started_at: started,
        entries: Arc::from([(key, 8)]),
    };
    let mut tracker = FullViewRemeshTracker::default();
    assert!(tracker.start(Some(&binding), exact_destination_status(), manifest, 90,));
    let first_expectation = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            ForcedRemeshManifestState::Complete,
            Some(proposed_render_expectation(
                started + Duration::from_millis(10),
                [(key, 8)],
            )),
            started + Duration::from_millis(10),
            91,
        )
        .unwrap();
    assert_eq!(
        tracker.observe_presented_frame(
            presented_acknowledgement(
                &first_expectation,
                43,
                Duration::from_millis(20),
                Duration::from_millis(40),
            ),
            92,
        ),
        None
    );

    let mut busy = settled_teleport_snapshot();
    busy.work.pending_mesh_jobs = 1;
    assert_eq!(
        tracker.reconcile_presented_expectation(
            busy,
            ForcedRemeshManifestState::Complete,
            Some(proposed_render_expectation(
                started + Duration::from_millis(50),
                [(key, 8)],
            )),
            started + Duration::from_millis(50),
            93,
        ),
        None
    );

    let resumed_expectation = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            ForcedRemeshManifestState::Complete,
            Some(proposed_render_expectation(
                started + Duration::from_millis(60),
                [(key, 8)],
            )),
            started + Duration::from_millis(60),
            94,
        )
        .unwrap();
    assert_ne!(
        resumed_expectation.render_ready_at, first_expectation.render_ready_at,
        "resumed proof reused the pre-gap render-ready boundary"
    );
    assert_eq!(
        tracker.observe_presented_frame(
            presented_acknowledgement(
                &resumed_expectation,
                44,
                Duration::from_millis(20),
                Duration::from_millis(40),
            ),
            95,
        ),
        None,
        "the first post-gap exact frame paired with the pre-gap frame"
    );
}

#[test]
fn cohort_or_manifest_change_invalidates_forced_remesh() {
    let teleport_started = Instant::now();
    let binding = binding_teleport_completion(teleport_started, Duration::from_millis(1_500));
    let started = teleport_started + Duration::from_millis(1_501);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let manifest = ForcedRemeshManifest {
        started_at: started,
        entries: Arc::from([(key, 8)]),
    };
    let proposal = proposed_render_expectation(started + Duration::from_millis(10), [(key, 8)]);

    let mut cohort_changed = FullViewRemeshTracker::default();
    assert!(cohort_changed.start(
        Some(&binding),
        exact_destination_status(),
        manifest.clone(),
        90,
    ));
    assert!(
        cohort_changed
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                ForcedRemeshManifestState::Complete,
                Some(proposal.clone()),
                started + Duration::from_millis(10),
                91,
            )
            .is_some()
    );
    let mut replacement = settled_teleport_snapshot();
    replacement.cohort.as_mut().unwrap().resident_hash ^= 0x55aa;
    assert!(
        cohort_changed
            .reconcile_presented_expectation(
                replacement,
                ForcedRemeshManifestState::Complete,
                Some(proposal.clone()),
                started + Duration::from_millis(20),
                92,
            )
            .is_none()
    );
    assert!(cohort_changed.is_invalidated());

    let mut manifest_changed = FullViewRemeshTracker::default();
    assert!(manifest_changed.start(Some(&binding), exact_destination_status(), manifest, 90,));
    assert!(
        manifest_changed
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                ForcedRemeshManifestState::Complete,
                Some(proposal),
                started + Duration::from_millis(10),
                91,
            )
            .is_some()
    );
    assert!(
        manifest_changed
            .reconcile_presented_expectation(
                settled_teleport_snapshot(),
                ForcedRemeshManifestState::Complete,
                Some(proposed_render_expectation(
                    started + Duration::from_millis(20),
                    [(key, 9)],
                )),
                started + Duration::from_millis(20),
                92,
            )
            .is_none()
    );
    assert!(manifest_changed.is_invalidated());

    let mut presented_changed = FullViewRemeshTracker::default();
    let manifest = ForcedRemeshManifest {
        started_at: started,
        entries: Arc::from([(key, 8)]),
    };
    assert!(presented_changed.start(Some(&binding), exact_destination_status(), manifest, 90,));
    let expectation = presented_changed
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            ForcedRemeshManifestState::Complete,
            Some(proposed_render_expectation(
                started + Duration::from_millis(10),
                [(key, 8)],
            )),
            started + Duration::from_millis(10),
            91,
        )
        .unwrap();
    let mut changed_ack = presented_acknowledgement(
        &expectation,
        43,
        Duration::from_millis(20),
        Duration::from_millis(40),
    );
    changed_ack.allocation_manifest = Arc::from([(key, 9)]);
    changed_ack.drawn_manifest = Arc::clone(&changed_ack.allocation_manifest);
    assert_eq!(
        presented_changed.observe_presented_frame(changed_ack, 92),
        None
    );
    assert!(
        presented_changed.is_invalidated(),
        "a forced-generation change in presented evidence did not invalidate the benchmark"
    );
}

#[test]
fn forced_remesh_start_rejects_entries_outside_the_frozen_allocation_manifest() {
    let teleport_started = Instant::now();
    let binding = binding_teleport_completion(teleport_started, Duration::from_millis(1_500));
    let started = teleport_started + Duration::from_millis(1_501);
    let key = binding.expectation.manifest[0].0;
    let extra = SubChunkKey::new(key.dimension, key.x + 1, key.y, key.z);
    let manifest = ForcedRemeshManifest {
        started_at: started,
        entries: Arc::from([(key, 8), (extra, 9)]),
    };

    let mut tracker = FullViewRemeshTracker::default();
    assert!(
        !tracker.start(Some(&binding), exact_destination_status(), manifest, 90),
        "forced-remesh proof must reject keys outside the frozen allocation cohort"
    );
}

#[test]
fn deterministic_mutation_coordinate_is_visible_above_the_surface_anchor() {
    assert_eq!(
        deterministic_mutation_coordinate([10.5, 72.62, -5.5], [10, -6]),
        [14, 71, -6]
    );
}

#[test]
fn remote_acceptance_anchors_the_mutation_to_the_live_player_not_world_spawn() {
    assert_eq!(
        acceptance_surface_anchor([-1351.25, 92.62, 1647.75]),
        [-1352, 1647]
    );
}

#[test]
fn mutation_look_target_centers_the_block_before_world_ready_visibility_sampling() {
    assert_eq!(
        mutation_look_target(Some([14, 71, -6])),
        Some(Vec3::new(14.5, 71.5, -5.5))
    );
    assert_eq!(mutation_look_target(None), None);
}

#[test]
fn acceptance_orients_a_normal_camera_toward_the_mutation_before_readiness() {
    let mut camera = Transform::from_xyz(10.5, 73.0, -5.5);
    assert!(orient_mutation_camera(&mut camera, Some([14, 71, -6])));
    let forward = camera.rotation * Vec3::NEG_Z;
    let expected = (Vec3::new(14.5, 71.5, -5.5) - camera.translation).normalize();
    assert!(forward.abs_diff_eq(expected, 0.0001));
}

#[test]
fn world_ready_markers_require_radius_rendering_and_include_the_exact_coordinate() {
    let mut snapshot = settled_world_snapshot();
    snapshot.received_radius_chunks = Some(17);
    assert_eq!(world_ready_markers(snapshot), None);
    snapshot.received_radius_chunks = Some(16);
    snapshot.publisher_radius_chunks = Some(8);
    snapshot.rendered_sub_chunks = 0;
    assert_eq!(world_ready_markers(snapshot), None);
    snapshot.rendered_sub_chunks = 2;
    assert_eq!(
        world_ready_markers(snapshot),
        Some([
            format!("{MUTATION_COORDINATE}=14,71,-6"),
            format!("{WORLD_READY} radius=16 rendered=2 resident=3 visible=1"),
        ])
    );

    snapshot.publisher_radius_chunks = Some(17);
    assert!(world_ready_markers(snapshot).is_some());

    snapshot.cohort = None;
    assert_eq!(world_ready_markers(snapshot), None);
}

#[test]
fn world_ready_binds_to_all_177_members_of_a_raw_120_publisher_cohort() {
    let target = ViewCohort::from_publisher(0, [-1350, 80, 1634], 120);
    let mut status = ViewCohortStatus {
        target,
        committed: Some(target),
        publisher_epoch: 1,
        expected: 177,
        required_hash: 0x120,
        loaded_target: 177,
        missing_target: 0,
        foreign_loaded: 0,
        foreign_requested: 0,
        foreign_resident: 0,
        source_leftover: 0,
        resident_count: 3,
        resident_hash: 1,
        known_air_count: 0,
        known_air_hash: 0,
    };
    let mut snapshot = settled_world_snapshot();
    snapshot.received_radius_chunks = Some(16);
    snapshot.publisher_radius_chunks = Some(8);
    snapshot.cohort = Some(status);

    assert!(world_ready_markers(snapshot).is_some());

    status.loaded_target = 176;
    status.missing_target = 1;
    snapshot.cohort = Some(status);
    assert_eq!(world_ready_markers(snapshot), None);
}

#[test]
fn teleport_readiness_accepts_the_servers_smaller_authoritative_publisher_disk() {
    let mut snapshot = settled_teleport_snapshot();
    snapshot.publisher_radius_chunks = Some(8);
    assert!(snapshot.is_binding_ready());

    snapshot.publisher_radius_chunks = Some(17);
    assert!(snapshot.is_binding_ready());

    snapshot.cohort.as_mut().unwrap().missing_target = 1;
    assert!(!snapshot.is_binding_ready());
}

#[test]
fn readiness_accepts_a_server_capped_radius_acknowledgement() {
    let mut snapshot = settled_world_snapshot();
    snapshot.received_radius_chunks = Some(8);
    snapshot.publisher_radius_chunks = Some(8);
    assert!(world_ready_markers(snapshot).is_some());

    let mut teleport = settled_teleport_snapshot();
    teleport.received_radius_chunks = Some(8);
    teleport.publisher_radius_chunks = Some(8);
    assert!(teleport.is_binding_ready());
}

#[test]
fn gallery_anchor_is_one_shot_mode_scoped_and_only_requires_the_clean_rendered_target() {
    let mut emitter = GalleryAnchorEmitter::default();
    let mut snapshot = settled_world_snapshot();
    snapshot.received_radius_chunks = None;
    snapshot.publisher_radius_chunks = None;
    snapshot.resident_sub_chunks = 0;
    snapshot.visible_sub_chunks = 0;
    snapshot.work.pending_mesh_jobs = 99;

    assert_eq!(emitter.observe(false, snapshot), None);

    snapshot.mutation_target_rendered = false;
    assert_eq!(emitter.observe(true, snapshot), None);
    snapshot.mutation_target_rendered = true;
    snapshot.mutation_target_visible = false;
    snapshot.mutation_target_clean = false;
    assert_eq!(emitter.observe(true, snapshot), None);
    snapshot.mutation_target_clean = true;

    assert_eq!(
        emitter.observe(true, snapshot),
        Some(
            format!(
                "{GALLERY_ANCHOR_READY} coordinate=14,71,-6 rendered=true visible=false clean=true"
            )
            .to_owned()
        )
    );
    assert_eq!(emitter.observe(true, snapshot), None);
}

#[test]
fn world_ready_markers_are_withheld_for_every_pending_stage_and_an_unclean_target() {
    let pending_stages = [
        (
            "network ingress",
            WorldReadyWork {
                network_events: 1,
                ..Default::default()
            },
        ),
        (
            "network commands",
            WorldReadyWork {
                network_commands: 1,
                ..Default::default()
            },
        ),
        (
            "admitted world events",
            WorldReadyWork {
                admitted_world_events: 1,
                ..Default::default()
            },
        ),
        (
            "queued decode",
            WorldReadyWork {
                queued_decode_jobs: 1,
                ..Default::default()
            },
        ),
        (
            "in-flight decode",
            WorldReadyWork {
                in_flight_decode_jobs: 1,
                ..Default::default()
            },
        ),
        (
            "completed decode",
            WorldReadyWork {
                completed_decode_results: 1,
                ..Default::default()
            },
        ),
        (
            "pending mesh",
            WorldReadyWork {
                pending_mesh_jobs: 1,
                ..Default::default()
            },
        ),
        (
            "pending light",
            WorldReadyWork {
                pending_light_jobs: 1,
                ..Default::default()
            },
        ),
        (
            "in-flight light",
            WorldReadyWork {
                in_flight_light_jobs: 1,
                ..Default::default()
            },
        ),
        (
            "terminal light failure",
            WorldReadyWork {
                terminal_light_failures: 1,
                ..Default::default()
            },
        ),
        (
            "in-flight mesh",
            WorldReadyWork {
                in_flight_mesh_jobs: 1,
                ..Default::default()
            },
        ),
        (
            "mesh changes",
            WorldReadyWork {
                pending_mesh_changes: 1,
                ..Default::default()
            },
        ),
        (
            "outbound requests",
            WorldReadyWork {
                outbound_requests: 1,
                ..Default::default()
            },
        ),
        (
            "outstanding sub-chunks",
            WorldReadyWork {
                outstanding_sub_chunks: 1,
                ..Default::default()
            },
        ),
        (
            "retry requests",
            WorldReadyWork {
                pending_retry_requests: 1,
                ..Default::default()
            },
        ),
        (
            "render queue",
            WorldReadyWork {
                render_queue_items: 1,
                ..Default::default()
            },
        ),
        (
            "GPU acknowledgements",
            WorldReadyWork {
                pending_gpu_acknowledgements: 1,
                ..Default::default()
            },
        ),
        (
            "unacknowledged meshes",
            WorldReadyWork {
                unacknowledged_meshes: 1,
                ..Default::default()
            },
        ),
    ];
    for (stage, work) in pending_stages {
        let mut snapshot = settled_world_snapshot();
        snapshot.work = work;
        assert_eq!(world_ready_markers(snapshot), None, "pending {stage}");
    }

    let mut target_not_rendered = settled_world_snapshot();
    target_not_rendered.mutation_target_rendered = false;
    assert_eq!(world_ready_markers(target_not_rendered), None);

    let mut target_not_visible = settled_world_snapshot();
    target_not_visible.mutation_target_visible = false;
    assert_eq!(world_ready_markers(target_not_visible), None);

    let mut target_not_clean = settled_world_snapshot();
    target_not_clean.mutation_target_clean = false;
    assert_eq!(world_ready_markers(target_not_clean), None);
}

#[test]
fn world_ready_requires_a_stable_quiet_interval_and_resets_when_work_reappears() {
    let started = Instant::now();
    let snapshot = settled_world_snapshot();
    let mut settler = WorldReadySettler::default();

    assert_eq!(settler.observe(snapshot, started), None);
    assert_eq!(
        settler.observe(
            snapshot,
            started + WORLD_READY_QUIET_INTERVAL - Duration::from_millis(1)
        ),
        None
    );

    let mut busy = snapshot;
    busy.work.pending_mesh_jobs = 1;
    assert_eq!(
        settler.observe(busy, started + WORLD_READY_QUIET_INTERVAL),
        None
    );

    let restarted = started + WORLD_READY_QUIET_INTERVAL + Duration::from_millis(1);
    assert_eq!(settler.observe(snapshot, restarted), None);
    let mut changed = snapshot;
    changed.rendered_sub_chunks += 1;
    assert_eq!(
        settler.observe(changed, restarted + WORLD_READY_QUIET_INTERVAL),
        None,
        "a changing candidate is not yet stable"
    );
    assert_eq!(
        settler.observe(changed, restarted + WORLD_READY_QUIET_INTERVAL * 2),
        world_ready_markers(changed)
    );
}

#[test]
fn mutation_tracker_closes_latency_only_on_the_target_gpu_acknowledgement() {
    let coordinate = [14, 71, -6];
    let observed_at = Instant::now();
    let mut tracker = MutationTracker::armed(coordinate, observed_at);
    let target_update = WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
        dimension: 0,
        position: coordinate,
        layer: 0,
        network_id: 7,
    }]);
    assert!(tracker.observe(&target_update, observed_at));

    let target_key = SubChunkKey::new(0, 0, 4, -1);
    assert_eq!(
        tracker.acknowledge(
            SubChunkKey::new(0, 1, 4, -1),
            observed_at,
            observed_at + Duration::from_millis(25),
        ),
        None
    );
    assert_eq!(
        tracker.acknowledge(
            target_key,
            observed_at - Duration::from_millis(1),
            observed_at + Duration::from_millis(25),
        ),
        None
    );
    assert_eq!(
        tracker.acknowledge(
            target_key,
            observed_at + Duration::from_millis(1),
            observed_at + Duration::from_millis(75),
        ),
        Some(Duration::from_millis(75))
    );
    assert_eq!(tracker.visible_count(), 1);
}

#[test]
fn full_view_mutation_closes_only_on_the_target_presented_generation() {
    let coordinate = [1_040, 58, 1_052];
    let key = SubChunkKey::new(0, 65, 3, 65);
    let armed_at = Instant::now();
    let observed_at = armed_at + Duration::from_millis(10);
    let render_ready_at = armed_at + Duration::from_millis(20);
    let mut tracker = MutationTracker::armed(coordinate, armed_at);
    let source_update = WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
        dimension: 0,
        position: [4, 70, -2],
        layer: 0,
        network_id: 7,
    }]);
    let target_update = WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
        dimension: 0,
        position: coordinate,
        layer: 0,
        network_id: 7,
    }]);

    assert!(!tracker.observe(&source_update, observed_at));
    assert!(!tracker.observe(&target_update, armed_at - Duration::from_millis(1)));
    assert!(tracker.observe(&target_update, observed_at));
    assert_eq!(
        tracker.acknowledge_upload(
            key,
            77,
            observed_at,
            observed_at + Duration::from_millis(5),
            true,
        ),
        None,
        "an upload acknowledgement settled a full-view mutation before presentation"
    );

    let expectation = tracker
        .reconcile_presented_expectation(
            proposed_render_expectation(render_ready_at, [(key, 77)]),
            8,
            render_ready_at,
        )
        .expect("the uploaded target generation should freeze an exact expectation");
    assert_eq!(expectation.view_generation, 9);
    let mut wrong_generation = presented_acknowledgement(
        &expectation,
        90,
        Duration::from_millis(10),
        Duration::from_millis(20),
    );
    wrong_generation.allocation_manifest = Arc::from([(key, 76)]);
    wrong_generation.drawn_manifest = Arc::from([(key, 76)]);
    assert_eq!(tracker.observe_presented_frame(wrong_generation), None);

    let latency = tracker
        .observe_presented_frame(presented_acknowledgement(
            &expectation,
            91,
            Duration::from_millis(10),
            Duration::from_millis(20),
        ))
        .expect("the exact target generation should close on GPU-completed presentation");
    assert_eq!(latency, Duration::from_millis(30));
    assert_eq!(tracker.visible_count(), 1);
}

#[test]
fn full_outbound_queue_retries_the_same_request_then_preserves_fifo_order() {
    let mut stream = client_world::WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    for (sequence, x) in [(1, 0), (2, 1), (3, 2)] {
        stream
            .submit(
                sequence,
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x,
                    z: 0,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: overworld_biome_payload(),
                }),
            )
            .unwrap();
    }
    complete_world_stream_decodes(&mut stream);

    let mut attempts = Vec::new();
    let mut calls = 0;
    let sent = flush_sub_chunk_requests(&mut stream, 8, |chunk, _, _, packet| {
        attempts.push(chunk.x);
        calls += 1;
        if calls == 2 {
            Err(crate::runtime::network::session::PacketSendError::Full(
                packet,
            ))
        } else {
            Ok(())
        }
    })
    .unwrap();
    assert_eq!(sent, 1);
    assert_eq!(stream.pending_request_count(), 2);
    stream.acknowledge_sub_chunk_request_sent(
        SubChunkKey::new(0, 0, -4, 0).chunk(),
        -4,
        1,
        Instant::now(),
    );
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 1);

    let sent = flush_sub_chunk_requests(&mut stream, 8, |chunk, _, _, _packet| {
        attempts.push(chunk.x);
        Ok(())
    })
    .unwrap();
    assert_eq!(sent, 2);
    assert_eq!(attempts, [0, 1, 1, 2]);
    assert_eq!(stream.pending_request_count(), 0);
    for x in [1, 2] {
        stream.acknowledge_sub_chunk_request_sent(
            SubChunkKey::new(0, x, -4, 0).chunk(),
            -4,
            1,
            Instant::now(),
        );
    }
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 3);
}

#[test]
fn command_admission_leaves_deadline_unarmed_until_transport_success_acknowledgement() {
    let request_stream = || {
        let mut stream = client_world::WorldStream::new(WorldBootstrap {
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
                WorldEvent::LevelChunk(LevelChunkEvent {
                    dimension: 0,
                    x: 0,
                    z: 0,
                    mode: LevelChunkMode::LimitedRequests { highest: 1 },
                    payload: overworld_biome_payload(),
                }),
            )
            .unwrap();
        complete_world_stream_decodes(&mut stream);
        stream
    };
    let key = SubChunkKey::new(0, 0, -4, 0);
    let mut stream = request_stream();

    assert_eq!(
        flush_sub_chunk_requests(&mut stream, 1, |_, _, _, _| Ok(())).unwrap(),
        1
    );
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
    let acknowledged_at = Instant::now() + Duration::from_secs(100);

    stream.acknowledge_sub_chunk_request_sent(key.chunk(), key.y, 1, acknowledged_at);
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 1);

    let mut failed = request_stream();
    assert_eq!(
        flush_sub_chunk_requests(&mut failed, 1, |_, _, _, packet| {
            Err(crate::runtime::network::session::PacketSendError::Full(
                packet,
            ))
        })
        .unwrap(),
        0
    );
    assert_eq!(failed.stats().awaiting_sub_chunk_responses, 0);
    assert_eq!(failed.stats().sub_chunk_timeouts, 0);
}

#[test]
fn network_session_fatal_is_retained_when_command_sender_closes_in_same_frame() {
    let mut stream = client_world::WorldStream::new(WorldBootstrap {
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
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: overworld_biome_payload(),
            }),
        )
        .unwrap();
    complete_world_stream_decodes(&mut stream);
    let original = "network session failed: Protocol error: original fatal";
    let mut fatal_error = None;
    record_fatal_error(&mut fatal_error, original.to_owned());

    let closed = flush_sub_chunk_requests(&mut stream, 1, |_, _, _, packet| {
        Err(crate::runtime::network::session::PacketSendError::Closed(
            packet,
        ))
    })
    .unwrap_err();
    record_fatal_error(&mut fatal_error, closed);

    assert_eq!(fatal_error.as_deref(), Some(original));
    assert_eq!(stream.pending_request_count(), 1);
    assert_eq!(stream.stats().awaiting_sub_chunk_responses, 0);
}

#[test]
fn exact_light_fatal_message_is_stage_independent() {
    let fatal = WorldStreamFatalError::LightSolve {
        key: SubChunkKey::new(0, -3, 12, 9),
        error: LightSolveError::QueueLimitExceeded { max: 1_000_000 },
    };
    let expected = "world stream fatal: light solve failed for SubChunkKey { dimension: 0, x: -3, y: 12, z: 9 }: light solve queue exceeded limit 1000000";

    for _world_ready in [false, true] {
        let mut fatal_error = None;
        record_fatal_error(&mut fatal_error, world_stream_fatal_message(fatal));
        assert_eq!(fatal_error.as_deref(), Some(expected));
    }
}

#[test]
fn zero_world_admission_still_drains_control_ack_and_leaves_world_fifo_untouched() {
    let (control_sender, mut control_receiver) = tokio::sync::mpsc::channel(1);
    let sent_at = Instant::now();
    control_sender
        .try_send(NetworkControlEvent::SubChunkRequestSent {
            chunk: ChunkKey::new(0, 3, -2),
            base_sub_chunk_y: -4,
            count: 24,
            sent_at,
        })
        .unwrap();
    let (world_sender, mut world_receiver) = tokio::sync::mpsc::channel(1);
    world_sender
        .try_send(SequencedWorldEvent {
            session_generation: 7,
            sequence: 1,
            event: WorldEvent::ChunkRadiusUpdated(16),
        })
        .unwrap();

    let controls = drain_network_controls(&mut control_receiver, OUTBOUND_SEND_BUDGET_PER_FRAME);
    let world = drain_network_ingress(&mut world_receiver, NETWORK_INGRESS_BUDGET_PER_FRAME.min(0));

    assert!(matches!(
        controls.as_slice(),
        [NetworkControlEvent::SubChunkRequestSent {
            chunk,
            base_sub_chunk_y: -4,
            count: 24,
            sent_at: observed,
        }] if *chunk == ChunkKey::new(0, 3, -2) && *observed == sent_at
    ));
    assert!(world.is_empty());
    assert!(matches!(
        world_receiver.try_recv(),
        Ok(SequencedWorldEvent {
            session_generation: 7,
            sequence: 1,
            event: WorldEvent::ChunkRadiusUpdated(16),
        })
    ));
}

#[test]
fn control_ingress_is_bounded_to_outbound_budget_and_preserves_fifo() {
    assert_eq!(OUTBOUND_SEND_BUDGET_PER_FRAME, 16);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(OUTBOUND_SEND_BUDGET_PER_FRAME + 2);
    for value in 0..OUTBOUND_SEND_BUDGET_PER_FRAME + 2 {
        sender.try_send(value).unwrap();
    }

    let drained = drain_network_controls(&mut receiver, OUTBOUND_SEND_BUDGET_PER_FRAME);

    assert_eq!(
        drained,
        (0..OUTBOUND_SEND_BUDGET_PER_FRAME).collect::<Vec<_>>()
    );
    assert_eq!(receiver.try_recv(), Ok(OUTBOUND_SEND_BUDGET_PER_FRAME));
    assert_eq!(receiver.try_recv(), Ok(OUTBOUND_SEND_BUDGET_PER_FRAME + 1));
}

#[test]
fn world_ingress_matches_heavy_admission_window_and_preserves_fifo() {
    assert_eq!(NETWORK_INGRESS_BUDGET_PER_FRAME, 32);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(NETWORK_INGRESS_BUDGET_PER_FRAME + 2);
    for value in 0..NETWORK_INGRESS_BUDGET_PER_FRAME + 2 {
        sender.try_send(value).unwrap();
    }

    let drained = drain_network_ingress(&mut receiver, NETWORK_INGRESS_BUDGET_PER_FRAME);

    assert_eq!(
        drained,
        (0..NETWORK_INGRESS_BUDGET_PER_FRAME).collect::<Vec<_>>()
    );
    assert_eq!(receiver.try_recv(), Ok(NETWORK_INGRESS_BUDGET_PER_FRAME));
    assert_eq!(
        receiver.try_recv(),
        Ok(NETWORK_INGRESS_BUDGET_PER_FRAME + 1)
    );
}

#[test]
fn camera_sub_chunk_key_uses_floor_and_euclidean_chunks() {
    assert_eq!(
        camera_sub_chunk_key(2, Vec3::new(-0.1, -64.1, 16.0)),
        SubChunkKey::new(2, -1, -5, 1)
    );
}

#[test]
fn status_title_exposes_live_input_coordinates_for_acceptance() {
    let transform = Transform {
        translation: Vec3::new(1.25, 72.0, -8.5),
        rotation: Quat::from_rotation_y(0.5),
        ..Default::default()
    };
    let title = status_title(&transform, 42, 37, true, 59.94);

    assert!(title.contains("59.9 FPS"));
    assert!(title.contains("pos 1.25 72.00 -8.50"));
    assert!(title.contains("yaw 0.50"));
    assert!(title.contains("chunks 37/42"));
    assert!(title.contains("captured"));
}

#[test]
fn biome_blend_marker_is_acceptance_only_render_committed_and_deduplicated() {
    let disabled = AcceptanceRun::new(None, None, false, false);
    let enabled = AcceptanceRun::new(Some(60), None, false, false);
    assert!(!biome_blend_diagnostics_enabled(&disabled));
    assert!(biome_blend_diagnostics_enabled(&enabled));

    let key = SubChunkKey::new(0, 4, -4, 9);
    let tint_identity = ChunkBiomeTintIdentity::new(7, 11);
    let record = PackedBiomeRecord::fallback();
    let snapshot =
        CommittedBiomeBlendSnapshot::from_record(key, 17, tint_identity, [15, 0, 15], &record)
            .unwrap();
    let mut last_emitted = None;
    let marker = biome_blend_diagnostic_marker_if_changed(&mut last_emitted, snapshot)
        .expect("first committed identity emits");
    assert!(marker.starts_with(
        "BIOME_BLEND_COMMITTED stage=app_committed key=0,4,-4,9 generation=17 tint_stream=7 tint_revision=11 record_hash="
    ));
    assert!(marker.contains(" local=15,0,15 radius=1 denominator=9 samples="));
    assert!(marker.ends_with(
        "-1,-1:0:1;0,-1:0:1;1,-1:0:1;-1,0:0:1;0,0:0:1;1,0:0:1;-1,1:0:1;0,1:0:1;1,1:0:1"
    ));
    assert!(biome_blend_diagnostic_marker_if_changed(&mut last_emitted, snapshot).is_none());

    let moved =
        CommittedBiomeBlendSnapshot::from_record(key, 17, tint_identity, [14, 0, 15], &record)
            .unwrap();
    assert!(biome_blend_diagnostic_marker_if_changed(&mut last_emitted, moved).is_some());

    let replaced =
        CommittedBiomeBlendSnapshot::from_record(key, 18, tint_identity, [14, 0, 15], &record)
            .unwrap();
    assert!(biome_blend_diagnostic_marker_if_changed(&mut last_emitted, replaced).is_some());
}

#[test]
fn rolling_fps_uses_only_the_most_recent_second() {
    let mut fps = RollingFps::default();
    for _ in 0..60 {
        fps.record(Duration::from_secs_f64(1.0 / 60.0));
    }
    assert!((fps.value() - 60.0).abs() < 0.01);

    for _ in 0..30 {
        fps.record(Duration::from_secs_f64(1.0 / 30.0));
    }
    assert!((fps.value() - 30.0).abs() < 0.01);
}

#[test]
fn diagnostic_telemetry_refreshes_only_after_resident_attribution_changes() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let mut tracker = DiagnosticQuadTracker::default();
    let mut metrics = MetricsCollector::new();
    let mut revision = tracker.revision();

    assert!(refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics).is_none());
    tracker.upsert(
        key,
        DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(54),
            537_536_753,
            6,
        )]),
    );
    let marker = refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics)
        .expect("changed diagnostic residency emits one marker");
    assert!(marker.contains("diagnostic_attribution_top=54|0x200a28f1|minecraft:leaf_litter|6"));
    assert!(
        refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics).is_none(),
        "unchanged frames must not rebuild or re-emit diagnostic attribution"
    );
    let unchanged_revision = tracker.revision();
    tracker.upsert(
        key,
        DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(54),
            537_536_753,
            6,
        )]),
    );
    assert_eq!(tracker.revision(), unchanged_revision);
    assert!(
        refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics).is_none(),
        "an identical remesh must not increment revision or re-emit telemetry"
    );
    tracker.remove(key);
    let cleared = refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics)
        .expect("eviction publishes the cleared resident state");
    assert!(cleared.contains("diagnostic_attribution_total=0"));
}

#[test]
fn cumulative_counter_delta_tolerates_a_counter_reset() {
    assert_eq!(cumulative_counter_delta(9, 4), 5);
    assert_eq!(cumulative_counter_delta(2, 9), 2);
}

#[test]
fn bedrock_yaw_and_pitch_map_to_bevys_negative_z_camera() {
    let south = bedrock_camera_rotation(0.0, 0.0) * Vec3::NEG_Z;
    let west = bedrock_camera_rotation(90.0, 0.0) * Vec3::NEG_Z;
    let looking_down = bedrock_camera_rotation(180.0, 45.0) * Vec3::NEG_Z;

    assert!(south.abs_diff_eq(Vec3::Z, 0.0001));
    assert!(west.abs_diff_eq(Vec3::NEG_X, 0.0001));
    assert!(looking_down.y < -0.7);
}

#[test]
fn committed_movement_correction_updates_local_camera_position_and_rotation() {
    let correction = PlayerMovementCorrectionEvent {
        position: [27.5, 111.0, 91.5],
        delta: [0.25, -0.5, 0.75],
        pitch: -15.0,
        yaw: 90.0,
        on_ground: true,
        tick: 55,
    };
    let mut camera = Transform::default();
    let mut pending_surface_spawn = Some([3, 4]);

    apply_committed_control(
        CommittedControlEvent::PlayerMovementCorrection {
            sequence: 7,
            correction,
            resolved: client_world::ResolvedServerPosition {
                position: correction.position,
                surface_anchor: None,
            },
        },
        &mut camera,
        &mut pending_surface_spawn,
    );

    assert_eq!(camera.translation, Vec3::new(27.5, 111.0, 91.5));
    assert!(camera.rotation.abs_diff_eq(
        bedrock_camera_rotation(correction.yaw, correction.pitch),
        0.0001
    ));
    assert_eq!(pending_surface_spawn, None);
}
