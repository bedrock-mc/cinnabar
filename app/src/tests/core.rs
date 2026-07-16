use super::*;
#[test]
fn actor_render_source_uses_only_remote_actor_pose_and_roster_skin() {
    let skin = PlayerSkin::Standard(StandardSkin {
        width: 64,
        height: 64,
        rgba8: vec![23; 64 * 64 * 4].into(),
    });
    let actor = client_world::ActorSnapshot {
        unique_id: 9,
        runtime_id: 77,
        spawn_revision: 19,
        movement_revision: 23,
        kind: ActorKind::Player {
            uuid: [7; 16],
            username: "remote".into(),
        },
        position: [10.0, 64.0, -3.0],
        velocity: [0.0; 3],
        pitch: 15.0,
        yaw: 45.0,
        head_yaw: 60.0,
        body_yaw: 45.0,
        on_ground: Some(true),
        teleported: false,
        metadata: Default::default(),
        attributes: Default::default(),
        int_properties: Default::default(),
        float_properties: Default::default(),
    };
    let profile = client_world::PlayerProfile {
        unique_id: 9,
        username: "remote".into(),
        verified: true,
        skin,
    };

    let source = actor_render_source(&actor, Some(&profile));
    assert_eq!(source.runtime_id, 77);
    assert_eq!(source.unique_id, 9);
    assert_eq!(source.spawn_revision, 19);
    assert_eq!(source.movement_revision, 23);
    assert_eq!(source.position, [10.0, 64.0, -3.0]);
    assert_eq!(source.yaw_degrees, 45.0);
    assert_eq!(source.head_yaw_degrees, 60.0);
    assert_eq!(source.skin.unwrap().rgba8.as_ref(), &[23; 64 * 64 * 4]);
}

#[test]
fn app_exit_arms_bounded_shutdown_with_the_requested_exit_code() {
    let (terminated, termination) = mpsc::channel();
    let watchdog = ShutdownWatchdog::new(Duration::from_millis(10), move |code| {
        terminated.send(code).unwrap();
    });
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(watchdog)
        .add_systems(Update, arm_shutdown_watchdog);

    app.world_mut().write_message(AppExit::error());
    app.update();

    assert_eq!(termination.recv_timeout(Duration::from_secs(1)), Ok(1));
}

#[test]
fn native_window_close_arms_watchdog_in_the_close_system() {
    let (terminated, termination) = mpsc::channel();
    let watchdog = ShutdownWatchdog::new(Duration::from_millis(10), move |code| {
        terminated.send(code).unwrap();
    });
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<WindowCloseRequested>()
        .insert_resource(watchdog)
        .add_systems(Update, exit_on_window_close_requested);
    let window = app.world_mut().spawn_empty().id();

    app.world_mut()
        .write_message(WindowCloseRequested { window });
    app.update();

    assert_eq!(app.should_exit(), Some(AppExit::Success));
    assert_eq!(termination.recv_timeout(Duration::from_secs(1)), Ok(0));
}

#[test]
fn completed_shutdown_cancels_watchdog_escalation() {
    let (terminated, termination) = mpsc::channel();
    let watchdog = ShutdownWatchdog::new(Duration::from_millis(30), move |code| {
        terminated.send(code).unwrap();
    });

    assert!(watchdog.arm(AppExit::Success));
    watchdog.complete();

    assert!(
        termination
            .recv_timeout(Duration::from_millis(100))
            .is_err()
    );
}

#[test]
fn shutdown_watchdog_arms_only_once() {
    let (terminated, termination) = mpsc::channel();
    let watchdog = ShutdownWatchdog::new(Duration::from_millis(10), move |code| {
        terminated.send(code).unwrap();
    });

    assert!(watchdog.arm(AppExit::Success));
    assert!(!watchdog.arm(AppExit::error()));

    assert_eq!(termination.recv_timeout(Duration::from_secs(1)), Ok(0));
    assert!(termination.recv_timeout(Duration::from_millis(50)).is_err());
}

#[test]
fn window_close_request_exits_before_the_window_is_despawned() {
    assert_eq!(window_close_exit(false), None);
    assert_eq!(window_close_exit(true), Some(AppExit::Success));
}

fn diagnostic_test_mesh() -> ChunkMesh {
    ChunkMesh::from_streams(
        Vec::new(),
        vec![PackedModelRef::new(0, 0, 0, 1)],
        vec![PackedQuadLighting::default()],
        vec![PackedModelDrawRef::new(0, 0)],
        Vec::new(),
        Vec::new(),
        FaceConnectivity::none(),
    )
}

#[test]
fn unavailable_visibility_digest_marker_fields_are_explicitly_invalid() {
    assert_eq!(
        visibility_digest_marker_fields("submitted", None),
        "submitted_valid=false submitted_count=null submitted_hash=null"
    );
}

#[test]
fn acceptance_present_mode_is_fifo_unless_no_vsync_is_explicit() {
    assert_eq!(requested_present_mode(false), PresentMode::Fifo);
    assert_eq!(requested_present_mode(true), PresentMode::Immediate);
}

#[test]
fn runtime_metadata_marker_records_build_presentation_and_adapter_identity() {
    let marker = acceptance_runtime_metadata_marker(
        AcceptanceRuntimeConfig {
            build_profile: "release",
        },
        &render::GraphicsAdapterMetadata {
            backend: "Dx12".to_owned(),
            adapter: "Test Adapter".to_owned(),
            driver: "test-driver".to_owned(),
            driver_info: "1.2.3".to_owned(),
            requested_present_mode: "Immediate".to_owned(),
            effective_present_mode: "Fifo".to_owned(),
            present_mode_proven: true,
        },
    );
    let encoded = marker
        .strip_prefix(&format!("{ACCEPTANCE_RUNTIME_METADATA}="))
        .unwrap();
    let document: serde_json::Value = serde_json::from_str(encoded).unwrap();

    assert_eq!(document["build_profile"], "release");
    assert_eq!(document["requested_present_mode"], "Immediate");
    assert_eq!(document["effective_present_mode"], "Fifo");
    assert_eq!(document["present_mode_proven"], true);
    assert_eq!(document["backend"], "Dx12");
    assert_eq!(document["adapter"], "Test Adapter");
    assert_eq!(document["driver"], "test-driver");
    assert_eq!(document["driver_info"], "1.2.3");
}

#[test]
fn world_publication_snapshot_is_deterministic_and_keeps_stage_identities_separate() {
    let stats = WorldStreamStats {
        accepted_light_jobs: u64::MAX,
        noop_light_jobs: 2,
        value_changed_light_jobs: 3,
        provenance_only_light_jobs: 5,
        light_mesh_invalidations: 7,
        stale_light_jobs: 11,
        stale_mesh_jobs: 13,
        queued_decode_jobs: 17,
        in_flight_decode_jobs: 19,
        pending_light_jobs: 23,
        in_flight_light_jobs: 29,
        pending_mesh_jobs: 31,
        in_flight_mesh_jobs: 37,
        max_decode_queue_wait: Duration::from_millis(41),
        max_light_queue_wait: Duration::from_millis(43),
        max_mesh_queue_wait: Duration::from_millis(47),
        max_decode_duration: Duration::from_millis(53),
        max_light_duration: Duration::from_millis(59),
        max_mesh_duration: Duration::from_millis(61),
        ..Default::default()
    };
    let visibility = VisibilityDiagnosticSnapshot {
        frame_generation: 67,
        pose_generation: 71,
        view_generation: 73,
        draw_mode: OpaqueDrawMode::Direct,
        ..Default::default()
    };
    let graphics = GraphicsAdapterMetadata {
        backend: "Dx12".to_owned(),
        adapter: "Test Adapter".to_owned(),
        driver: "test-driver".to_owned(),
        driver_info: "1.2.3".to_owned(),
        requested_present_mode: "Fifo".to_owned(),
        effective_present_mode: "Fifo".to_owned(),
        present_mode_proven: true,
    };

    let marker = world_publication_snapshot_marker(
        stats,
        79,
        83,
        89,
        visibility,
        AcceptanceRuntimeConfig {
            build_profile: "debug",
        },
        &graphics,
    );
    assert_eq!(
        marker,
        world_publication_snapshot_marker(
            stats,
            79,
            83,
            89,
            visibility,
            AcceptanceRuntimeConfig {
                build_profile: "debug",
            },
            &graphics,
        )
    );
    let document: serde_json::Value = serde_json::from_str(
        marker
            .strip_prefix(&format!("{WORLD_PUBLICATION_SNAPSHOT}="))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(document["accepted_light_jobs"], u64::MAX);
    assert_eq!(document["max_decode_queue_wait_ms"], 41.0);
    assert_eq!(document["max_decode_worker_ms"], 53.0);
    assert_eq!(document["upload_queue_items"], 79);
    assert_eq!(document["upload_queue_bytes"], 83);
    assert_eq!(document["gpu_upload_bytes"], 89);
    assert_eq!(document["frame_generation"], 67);
    assert_eq!(document["draw_mode"], "Direct");
    assert_eq!(document["build_profile"], "debug");
    assert_eq!(document["requested_present_mode"], "Fifo");
    assert_eq!(document["effective_present_mode"], "Fifo");
    assert_eq!(document["present_mode_proven"], true);
}

#[test]
fn visibility_capture_observes_post_deferred_upload_and_removal_generation() {
    let key = SubChunkKey::new(0, 3, -4, 5);
    let expected = VisibilityKeyDigest::from_keys([key]);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(CaveVisibilityCache::default())
        .insert_resource(VisibilityDiagnosticsInput::new(true))
        .add_plugins(ChunkRenderPlugin::new(1))
        .add_observer(apply_added_chunk_visibility)
        .add_observer(remove_chunk_visibility)
        .add_systems(
            Update,
            update_visibility_diagnostics.after(ChunkRenderApplySet),
        );

    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(key, diagnostic_test_mesh(), ChunkUploadPriority::new(0.0))
        .unwrap();
    app.update();
    let inserted = app.world().resource::<VisibilityDiagnosticsInput>();
    assert_eq!(inserted.resident_mesh(), Some(expected));
    assert_eq!(inserted.cave_visible(), Some(expected));

    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_remove(key)
        .unwrap();
    app.update();
    let removed = app.world().resource::<VisibilityDiagnosticsInput>();
    assert_eq!(
        removed.resident_mesh(),
        Some(VisibilityKeyDigest::default())
    );
    assert_eq!(removed.cave_visible(), Some(VisibilityKeyDigest::default()));
}

#[test]
fn camera_medium_sampling_is_ordered_after_fly_camera_transform_updates() {
    let camera_source = include_str!("../camera.rs");
    let main_source = include_str!("../app.rs");
    assert!(camera_source.contains(".in_set(FlyCameraUpdateSet)"));
    assert!(main_source.contains(".after(FlyCameraUpdateSet)"));
}

#[test]
fn camera_environment_context_is_sampled_before_profiled_atmosphere_derivation() {
    let main_source = include_str!("../app.rs");
    let environment_source = include_str!("../environment.rs");
    let world_source = include_str!("../runtime/world.rs");
    assert!(main_source.contains("insert_resource(EnvironmentContext::default())"));
    assert!(main_source.contains("insert_resource(EnvironmentProfileRoute::default())"));
    let sample = main_source.rfind("update_camera_medium,").unwrap();
    let derive = main_source.rfind("update_atmosphere_frame,").unwrap();
    assert!(sample < derive);
    assert!(world_source.contains(".camera_biome_id(camera.translation.to_array())"));
    assert!(world_source.contains(".render_distance_blocks()"));
    assert!(environment_source.contains("assets.biome_profiles()"));
    assert!(environment_source.contains("assets.fog_profiles()"));
}

pub(super) fn overworld_biome_payload() -> Vec<u8> {
    let mut payload = vec![1, 2];
    payload.extend(std::iter::repeat_n(0xff, 23));
    payload.push(0);
    payload
}

pub(super) fn complete_world_stream_decodes(stream: &mut WorldStream) {
    for _ in 0..10_000 {
        stream.poll([0.0; 3], 0);
        let stats = stream.stats();
        if stats.queued_decode_jobs == 0
            && stats.in_flight_decode_jobs == 0
            && stats.completed_decode_results == 0
        {
            return;
        }
        std::thread::yield_now();
    }
    panic!("world stream decode did not complete");
}

#[test]
fn client_world_publication_contract_crosses_the_app_boundary() {
    let mut stream = WorldStream::new(WorldBootstrap {
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

    let request = stream.pop_next_request().expect("sub-chunk request");
    stream.record_sub_chunk_request_transport_pending(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
    );
    stream.acknowledge_sub_chunk_request_sent(
        request.chunk,
        request.base_sub_chunk_y,
        request.count,
        Instant::now(),
    );
    let key = SubChunkKey::from_chunk(request.chunk, request.base_sub_chunk_y);
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![SubChunkEntryEvent {
                    position: [key.x, key.y, key.z],
                    result: SubChunkResult::AllAir,
                }],
            }),
        )
        .unwrap();
    complete_world_stream_decodes(&mut stream);

    let acknowledgement = (0..128)
        .find_map(|_| {
            stream.poll([0.0; 3], 1);
            let mut target = None;
            while let Some(change) = stream.pop_mesh_change() {
                let (changed, generation, dirty_since) = match change {
                    WorldMeshChange::Upsert {
                        key,
                        generation,
                        dirty_since,
                        ..
                    }
                    | WorldMeshChange::Remove {
                        key,
                        generation,
                        dirty_since,
                    } => (key, generation, dirty_since),
                };
                stream.acknowledge_mesh_upload(changed, generation, dirty_since, Instant::now());
                if changed == key {
                    target = Some((generation, dirty_since));
                }
            }
            std::thread::yield_now();
            target
        })
        .expect("public mesh publication");
    assert_ne!(acknowledgement.0, 0);
    assert!(stream.is_mesh_clean(key));
}

#[test]
fn compiled_and_live_biome_tables_preserve_raw_id_water_colour_parity() {
    let runtime_assets = Arc::new(RuntimeAssets::diagnostic());
    let mut active = startup_biome_tints(&runtime_assets);
    let initial = runtime_assets.biome_assets().resolve_live(&[]).unwrap();
    assert_eq!(active.entries().len(), initial.records.len());
    assert_eq!(active.revision(), 0);

    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 12_530,
            block_network_ids_are_hashes: false,
        },
        runtime_assets,
        [0.0, 96.0, 0.0],
        None,
    );
    stream
        .submit(
            1,
            WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                definitions: Arc::from([
                    BiomeDefinitionEvent {
                        biome_id: Some(42),
                        name: Arc::from("example:cool_water"),
                        temperature: 0.8,
                        downfall: 0.4,
                        snow_foliage: 0.0,
                        map_water_color: 0xff44_6688,
                    },
                    BiomeDefinitionEvent {
                        biome_id: Some(43),
                        name: Arc::from("example:warm_water"),
                        temperature: 0.8,
                        downfall: 0.4,
                        snow_foliage: 0.0,
                        map_water_color: 0xffaa_3300,
                    },
                ]),
            }),
        )
        .unwrap();

    assert!(synchronize_biome_tints(&stream, &mut active));
    assert_eq!(active.revision(), stream.biome_tint_revision());
    assert_eq!(active.entries().len(), 3);
    let resolved = stream.resolved_biome_tints_snapshot();
    let cool = usize::try_from(resolved.dense_index(42)).unwrap();
    let warm = usize::try_from(resolved.dense_index(43)).unwrap();
    assert_ne!(cool, warm);
    assert_eq!(
        active.entries()[cool].water,
        resolved.records[cool].water[..3]
    );
    assert_eq!(
        active.entries()[warm].water,
        resolved.records[warm].water[..3]
    );
    assert_ne!(active.entries()[cool].water, active.entries()[warm].water);
    assert!(!synchronize_biome_tints(&stream, &mut active));
}

#[test]
fn equal_numeric_revisions_from_different_streams_replace_the_active_table() {
    fn stream_with_live_temperature(
        runtime_assets: Arc<RuntimeAssets>,
        temperature: f32,
    ) -> WorldStream {
        let mut stream = WorldStream::new_with_assets(
            WorldBootstrap {
                dimension: 0,
                local_player_runtime_id: 1,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 12_530,
                block_network_ids_are_hashes: false,
            },
            runtime_assets,
            [0.0, 96.0, 0.0],
            None,
        );
        stream
            .submit(
                1,
                WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
                    definitions: Arc::from([BiomeDefinitionEvent {
                        biome_id: Some(42),
                        name: Arc::from("example:live"),
                        temperature,
                        downfall: 0.4,
                        snow_foliage: 0.0,
                        map_water_color: if temperature > 0.5 {
                            0xff11_2233
                        } else {
                            0xffaa_bbcc
                        },
                    }]),
                }),
            )
            .unwrap();
        stream
    }

    let runtime_assets = Arc::new(RuntimeAssets::diagnostic());
    let first = stream_with_live_temperature(Arc::clone(&runtime_assets), 0.8);
    let second = stream_with_live_temperature(runtime_assets, 0.2);
    assert_eq!(first.biome_tint_revision(), second.biome_tint_revision());

    let mut active = ChunkBiomeTints::default();
    assert!(synchronize_biome_tints(&first, &mut active));
    let first_entries = active.entries().to_vec();
    assert!(synchronize_biome_tints(&second, &mut active));
    assert_ne!(active.entries(), first_entries);
}

pub(super) fn settled_world_snapshot() -> WorldReadySnapshot {
    WorldReadySnapshot {
        mutation_coordinate: Some([14, 71, -6]),
        received_radius_chunks: Some(16),
        publisher_radius_chunks: Some(16),
        rendered_sub_chunks: 2,
        resident_sub_chunks: 3,
        visible_sub_chunks: 1,
        mutation_target_rendered: true,
        mutation_target_visible: true,
        mutation_target_clean: true,
        work: WorldReadyWork::default(),
    }
}

#[test]
fn acceptance_run_retains_the_spawn_surface_anchor_until_coordinate_resolution() {
    let mut acceptance = AcceptanceRun::new(Some(900), None, false, false);
    assert!(acceptance.enabled());
    acceptance.set_mutation_surface_anchor([10, -6]);
    assert_eq!(acceptance.mutation_surface_anchor(), Some([10, -6]));
    acceptance.set_mutation_coordinate([14, 71, -6]);
    assert_eq!(acceptance.mutation_surface_anchor(), None);
    assert_eq!(acceptance.mutation_coordinate(), Some([14, 71, -6]));
}

#[test]
fn full_view_move_player_ingress_marker_is_exact_and_nonbinding_events_are_silent() {
    let started = Instant::now();
    let mut acceptance = AcceptanceRun::new(Some(900), None, true, false);
    acceptance.set_mutation_coordinate([0, 58, 0]);
    acceptance.begin_world_ready(started, [0.5, 70.0, 0.5], 1);

    let publisher = WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
        center: [0, 70, 0],
        radius_blocks: 256,
    });
    let accepted = acceptance.observe_full_view_teleport_ingress(&publisher, 40, started, 0, 10);
    assert!(!accepted);
    assert_eq!(
        accepted_move_player_ingress_marker(accepted, 40, &publisher),
        None,
    );

    let near = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
        runtime_id: 1,
        position: [16.5, 70.0, 0.5],
        pitch: 0.0,
        yaw: 0.0,
    });
    let accepted = acceptance.observe_full_view_teleport_ingress(
        &near,
        41,
        started + Duration::from_millis(1),
        0,
        11,
    );
    assert!(!accepted);
    assert_eq!(
        accepted_move_player_ingress_marker(accepted, 41, &near),
        None,
    );

    let binding = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
        runtime_id: 1,
        position: [1_040.5, 93.75, 1_040.5],
        pitch: 0.0,
        yaw: 0.0,
    });
    let accepted = acceptance.observe_full_view_teleport_ingress(
        &binding,
        42,
        started + Duration::from_millis(2),
        0,
        12,
    );
    assert!(accepted);
    assert_eq!(
        accepted_move_player_ingress_marker(accepted, 42, &binding),
        Some(format!(
            "{MOVE_PLAYER_INGRESS} sequence=42 position=1040.5,93.75,1040.5"
        )),
    );
    assert_eq!(
        acceptance
            .full_view_teleport
            .pending_move_ingress
            .map(|(sequence, _, _)| sequence),
        Some(42),
    );
}

#[test]
fn full_view_move_player_ingress_marker_rejects_nonfinite_xz_but_preserves_y_independence() {
    let started = Instant::now();
    for position in [
        [f32::NAN, 70.0, 1_040.5],
        [1_040.5, 70.0, f32::NEG_INFINITY],
    ] {
        let mut acceptance = AcceptanceRun::new(Some(900), None, true, false);
        acceptance.set_mutation_coordinate([0, 58, 0]);
        acceptance.begin_world_ready(started, [0.5, 70.0, 0.5], 1);
        let movement = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position,
            pitch: 0.0,
            yaw: 0.0,
        });
        let accepted = acceptance.observe_full_view_teleport_ingress(&movement, 43, started, 0, 10);
        assert!(!accepted);
        assert_eq!(
            accepted_move_player_ingress_marker(accepted, 43, &movement),
            None,
            "nonfinite position {position:?} produced parser-visible ingress evidence",
        );
    }

    let mut acceptance = AcceptanceRun::new(Some(900), None, true, false);
    acceptance.set_mutation_coordinate([0, 58, 0]);
    acceptance.begin_world_ready(started, [0.5, 70.0, 0.5], 1);
    let movement = WorldEvent::MovePlayer(protocol::MovePlayerEvent {
        runtime_id: 1,
        position: [1_040.5, f32::INFINITY, 1_040.5],
        pitch: 0.0,
        yaw: 0.0,
    });
    let accepted = acceptance.observe_full_view_teleport_ingress(&movement, 43, started, 0, 10);
    assert!(
        accepted,
        "nonfinite MovePlayer Y changed binding acceptance"
    );
    assert_eq!(
        accepted_move_player_ingress_marker(accepted, 43, &movement),
        None,
        "nonfinite MovePlayer Y produced a non-parser-safe marker instead of only preserving capture",
    );
    assert_eq!(
        acceptance
            .full_view_teleport
            .pending_move_ingress
            .map(|(sequence, _, _)| sequence),
        Some(43),
    );
}

#[test]
fn move_player_ingress_marker_is_flushed_before_source_capture() {
    struct OrderingWriter {
        bytes: std::rc::Rc<std::cell::RefCell<Vec<u8>>>,
        flushed: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl std::io::Write for OrderingWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.bytes.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushed.set(true);
            Ok(())
        }
    }

    let bytes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let flushed = std::rc::Rc::new(std::cell::Cell::new(false));
    let capture_called = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut writer = OrderingWriter {
        bytes: std::rc::Rc::clone(&bytes),
        flushed: std::rc::Rc::clone(&flushed),
    };
    let capture_called_for_callback = std::rc::Rc::clone(&capture_called);
    let bytes_for_callback = std::rc::Rc::clone(&bytes);
    let flushed_for_callback = std::rc::Rc::clone(&flushed);

    write_move_player_ingress_before_source_capture(
        &mut writer,
        &format!("{MOVE_PLAYER_INGRESS} sequence=42 position=1040.5,93.75,1040.5"),
        || {
            assert!(flushed_for_callback.get());
            assert_eq!(
                bytes_for_callback.borrow().as_slice(),
                format!("{MOVE_PLAYER_INGRESS} sequence=42 position=1040.5,93.75,1040.5\n")
                    .as_bytes(),
            );
            capture_called_for_callback.set(true);
        },
    );

    assert!(capture_called.get());
}

#[test]
fn full_view_mutation_stays_disarmed_until_exact_target_and_remesh_binding() {
    let source = [0, 58, 0];
    let started = Instant::now();
    let mut acceptance = AcceptanceRun::new(Some(900), None, true, false);
    acceptance.set_mutation_coordinate(source);

    assert_eq!(acceptance.source_mutation_coordinate(), Some(source));
    assert!(acceptance.mutation.is_none());
    assert_eq!(acceptance.target_mutation_marker(), None);
    assert!(!acceptance.retarget_mutation([1_040, 58, 1_052], started));

    acceptance.full_view_teleport = destination_tracker(started);
    let key = SubChunkKey::new(0, 65, 64, 65);
    let expectation = acceptance
        .full_view_teleport
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposed_render_expectation(started + Duration::from_millis(200), [(key, 7)]),
            started + Duration::from_millis(200),
        )
        .unwrap();
    assert_eq!(
        acceptance
            .full_view_teleport
            .observe_presented_frame(presented_acknowledgement(
                &expectation,
                1,
                Duration::from_millis(10),
                Duration::from_millis(20),
            )),
        None
    );
    let completion = acceptance
        .full_view_teleport
        .observe_presented_frame(presented_acknowledgement(
            &expectation,
            2,
            Duration::from_millis(30),
            Duration::from_millis(40),
        ))
        .expect("the exact adjacent frame pair should bind the target");
    let target = [1_040, 58, 1_052];
    assert_eq!(completion.target_mutation_coordinate, target);
    assert!(
        !acceptance.retarget_mutation(target, completion.stable_frame.gpu_completed_at),
        "teleport binding armed mutation before the frozen forced remesh completed"
    );
    assert_eq!(acceptance.target_mutation_marker(), None);

    let remesh_started = completion.stable_frame.gpu_completed_at + Duration::from_millis(1);
    let manifest = ForcedRemeshManifest {
        started_at: remesh_started,
        entries: Arc::from([(key, 8)]),
    };
    assert!(acceptance.full_view_remesh.start(
        Some(&completion),
        exact_destination_status(),
        manifest,
        3,
    ));
    let remesh_expectation = acceptance
        .full_view_remesh
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            ForcedRemeshManifestState::Complete,
            Some(proposed_render_expectation(
                remesh_started + Duration::from_millis(10),
                [(key, 8)],
            )),
            remesh_started + Duration::from_millis(10),
            4,
        )
        .unwrap();
    assert_eq!(
        acceptance.full_view_remesh.observe_presented_frame(
            presented_acknowledgement(
                &remesh_expectation,
                3,
                Duration::from_millis(10),
                Duration::from_millis(20),
            ),
            4,
        ),
        None
    );
    let remesh_completion = acceptance
        .full_view_remesh
        .observe_presented_frame(
            presented_acknowledgement(
                &remesh_expectation,
                4,
                Duration::from_millis(30),
                Duration::from_millis(40),
            ),
            5,
        )
        .expect("the exact forced-remesh frame pair should settle");
    assert_eq!(acceptance.target_mutation_marker(), None);
    assert!(acceptance.retarget_mutation(target, remesh_completion.stable_frame.gpu_completed_at,));
    assert_eq!(
        acceptance.target_mutation_marker(),
        Some(format!(
            "{TARGET_MUTATION_ARMED} source=0,58,0 target=1040,58,1052 view_generation={}",
            remesh_completion.view_generation
        ))
    );
    assert_eq!(acceptance.mutation_coordinate(), Some(target));
    assert_eq!(acceptance.visible_mutation_count(), 0);
}

#[test]
fn leaf_forest_target_mutation_uses_the_binding_move_player_offset() {
    let source = [4, 70, -2];
    assert_eq!(
        leaf_forest_target_mutation_coordinate([1_044.5, 93.75, 1_038.5], source),
        Some([1_044, 70, 1_050])
    );
    assert_eq!(
        leaf_forest_target_mutation_coordinate([f32::NAN, 93.75, 1_038.5], source),
        None
    );
    assert_eq!(
        leaf_forest_target_mutation_coordinate([1_044.5, 93.75, f32::INFINITY], source),
        None
    );
    assert_eq!(
        leaf_forest_target_mutation_coordinate([1_044.5, f32::INFINITY, 1_038.5], source,),
        Some([1_044, 70, 1_050]),
        "target mutation Y must come from the manifest-compatible source coordinate"
    );
    assert_eq!(
        leaf_forest_target_mutation_coordinate([1_043.5, 93.75, 1_038.5], source),
        None,
        "a MovePlayer target outside the exact 65-chunk forest offset was accepted"
    );
}

#[test]
fn target_mutation_marker_is_exact_and_manifest_comparable() {
    assert_eq!(
        target_mutation_armed_marker([4, 70, -2], [1_044, 70, 1_050], 9),
        format!("{TARGET_MUTATION_ARMED} source=4,70,-2 target=1044,70,1050 view_generation=9")
    );
}

#[test]
fn timed_acceptance_deadline_begins_only_when_the_world_is_ready() {
    let mut acceptance = AcceptanceRun::new(Some(900), None, false, false);
    assert_eq!(acceptance.deadline, None);

    let world_ready_at = Instant::now() + Duration::from_secs(60);
    acceptance.begin_world_ready(world_ready_at, [0.5, 70.0, 0.5], 1);

    assert!(acceptance.world_ready);
    assert_eq!(
        acceptance.deadline,
        Some(world_ready_at + Duration::from_secs(900))
    );
}

fn settled_transparent_snapshot(generation: u64) -> TransparentSortMetricsSnapshot {
    TransparentSortMetricsSnapshot {
        committed_generation: generation,
        encoded_generation: generation,
        presented_generation: generation,
        ref_count: 42,
        ..Default::default()
    }
}

#[test]
fn timed_exit_completes_immediately_for_gpu_presented_transparent_snapshot() {
    let deadline = Instant::now();
    let mut acceptance = AcceptanceRun::new(Some(60), None, false, true);
    acceptance.deadline = Some(deadline);

    assert_eq!(
        acceptance.exit_decision(deadline, false, settled_transparent_snapshot(17)),
        AcceptanceExitDecision::Complete
    );
}

#[test]
fn timed_exit_does_not_impose_water_settle_on_opaque_acceptance() {
    let deadline = Instant::now();
    let mut acceptance = AcceptanceRun::new(Some(60), None, false, false);
    acceptance.deadline = Some(deadline);

    assert_eq!(
        acceptance.exit_decision(deadline, false, TransparentSortMetricsSnapshot::default(),),
        AcceptanceExitDecision::Complete
    );
}

#[test]
fn timed_exit_waits_for_delayed_gpu_presentation_within_grace() {
    let deadline = Instant::now();
    let mut acceptance = AcceptanceRun::new(Some(60), None, false, true);
    acceptance.deadline = Some(deadline);
    let pending = TransparentSortMetricsSnapshot {
        committed_generation: 18,
        encoded_generation: 18,
        presented_generation: 17,
        ref_count: 42,
        ..Default::default()
    };

    assert_eq!(
        acceptance.exit_decision(deadline, false, pending),
        AcceptanceExitDecision::WaitForTransparentPresentation
    );
    assert_eq!(
        acceptance.exit_decision(
            deadline + TRANSPARENT_PRESENTATION_EXIT_GRACE - Duration::from_millis(1),
            false,
            settled_transparent_snapshot(18),
        ),
        AcceptanceExitDecision::Complete
    );
}

#[test]
fn timed_exit_turns_unsettled_transparency_into_bounded_fatal_failure() {
    let deadline = Instant::now();
    let mut acceptance = AcceptanceRun::new(Some(60), None, false, true);
    acceptance.deadline = Some(deadline);
    let pending = TransparentSortMetricsSnapshot {
        committed_generation: 581,
        encoded_generation: 581,
        presented_generation: 0,
        ref_count: 42,
        ..Default::default()
    };

    assert_eq!(
        acceptance.exit_decision(
            deadline + TRANSPARENT_PRESENTATION_EXIT_GRACE,
            false,
            pending,
        ),
        AcceptanceExitDecision::TransparentPresentationTimedOut
    );
    assert!(AcceptanceExitDecision::TransparentPresentationTimedOut.is_error());
}

#[test]
fn fatal_error_exits_immediately_even_before_timed_deadline() {
    let now = Instant::now();
    let mut acceptance = AcceptanceRun::new(Some(60), None, false, true);
    acceptance.deadline = Some(now + Duration::from_secs(60));

    assert_eq!(
        acceptance.exit_decision(now, true, TransparentSortMetricsSnapshot::default()),
        AcceptanceExitDecision::Fatal
    );
    assert!(AcceptanceExitDecision::Fatal.is_error());
}

#[test]
fn interactive_network_failure_requests_exit_without_waiting_for_acceptance_finalization() {
    assert_eq!(
        fatal_runtime_exit("network session failed: bridge closed"),
        Some(AppExit::error())
    );
    assert_eq!(fatal_runtime_exit(""), None);
}
