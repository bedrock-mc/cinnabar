use std::time::Duration;

use bevy::{
    ecs::schedule::{
        IntoSystemSet, NodeId, ScheduleGraph, Schedules, SystemSet,
        graph::{DiGraph, Direction},
    },
    prelude::{App, Update},
};

use crate::app::{
    ClientFrameSet, configure_acceptance_finish_system, configure_client_frame_schedule,
    configure_client_production_frame_systems, configure_client_runtime_frame_systems,
};
use crate::block_use::produce_block_use;
use crate::item_use::produce_item_use;
use crate::local_player::{
    publish_interaction_origin, publish_local_player_frame, resolve_camera_pose,
};
use crate::melee::produce_melee;
use crate::movement::advance_local_physics;
use crate::runtime::network::{
    advance_actor_frame, prepare_actor_render_frame, publish_actor_render_frame,
    receive_network_events,
};
use crate::runtime::phase3_evidence::emit_phase3_evidence;
use crate::runtime::publication::{
    PublicationController, PublicationFrameWork, adaptive_publication_diagnostic_line,
    begin_publication_frame,
};
use crate::runtime::shutdown::finish_acceptance_run;
use crate::runtime::telemetry::send_player_auth_inputs;
use crate::runtime::world::{
    drive_world_stream, mesh_change_has_publication_permit, reconcile_world_stream_before_physics,
    update_camera_medium,
};
use crate::semantic_controls::{
    collect_raw_input, finalize_semantic_input_after_ui_authority, route_semantic_input,
    synchronize_semantic_input_authority,
};
use crate::session::recover_session_failure;
use crate::survival_mining::produce_survival_mining;
use crate::ui_runtime::presentation::{prepare_ui_runtime, publish_ui_runtime};
use chunk_pipeline::{PublicationServiceConfig, WorldMeshChange};

#[test]
fn production_client_systems_are_members_of_the_behavioral_sets() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    configure_client_production_frame_systems(&mut app);
    configure_client_runtime_frame_systems(&mut app);

    let schedules = app.world().resource::<Schedules>();
    let graph = schedules
        .get(Update)
        .expect("production Update schedule")
        .graph();
    let publication = system_node(graph, begin_publication_frame, "begin_publication_frame");
    let render_apply = NodeId::Set(
        graph
            .system_sets
            .get_key(render::ChunkRenderApplySet.intern())
            .unwrap(),
    );
    assert!(schedule_precedes(
        graph,
        system_node(graph, drive_world_stream, "drive_world_stream"),
        render_apply,
    ));
    for consumer in [
        system_node(graph, receive_network_events, "receive_network_events"),
        system_node(graph, drive_world_stream, "drive_world_stream"),
        render_apply,
    ] {
        assert!(schedule_precedes(graph, publication, consumer));
    }
    let fly_camera = NodeId::Set(
        graph
            .system_sets
            .get_key(crate::camera::FlyCameraUpdateSet.intern())
            .unwrap(),
    );
    let medium = system_node(graph, update_camera_medium, "update_camera_medium");
    let atmosphere = system_node(
        graph,
        crate::environment::update_atmosphere_frame,
        "update_atmosphere_frame",
    );
    assert!(schedule_precedes(graph, fly_camera, medium));
    assert!(schedule_precedes(graph, medium, atmosphere));
    assert!(!schedule_precedes(graph, fly_camera, publication));

    let stages = [
        ClientFrameSet::RawInput,
        ClientFrameSet::SemanticSample,
        ClientFrameSet::UiAuthority,
        ClientFrameSet::SemanticFinalize,
        ClientFrameSet::Physics,
        ClientFrameSet::Camera,
        ClientFrameSet::Interaction,
        ClientFrameSet::NetworkSend,
        ClientFrameSet::WorldPublication,
        ClientFrameSet::ActorPreparation,
        ClientFrameSet::UiPreparation,
        ClientFrameSet::ActorFinalization,
        ClientFrameSet::ActorPublication,
        ClientFrameSet::UiPublication,
    ];

    for adjacent in stages.windows(2) {
        assert!(
            schedule_precedes(
                graph,
                stage_node(graph, adjacent[0]),
                stage_node(graph, adjacent[1]),
            ),
            "{:?} must execute before {:?}",
            adjacent[0],
            adjacent[1]
        );
    }

    assert_system_in_stage(
        graph,
        collect_raw_input,
        "collect_raw_input",
        ClientFrameSet::RawInput,
    );
    assert_system_in_stage(
        graph,
        route_semantic_input,
        "route_semantic_input",
        ClientFrameSet::SemanticSample,
    );
    assert_system_in_stage(
        graph,
        synchronize_semantic_input_authority,
        "synchronize_semantic_input_authority",
        ClientFrameSet::UiAuthority,
    );
    assert_system_in_stage(
        graph,
        finalize_semantic_input_after_ui_authority,
        "finalize_semantic_input_after_ui_authority",
        ClientFrameSet::SemanticFinalize,
    );
    assert_system_in_stage(
        graph,
        advance_local_physics,
        "advance_local_physics",
        ClientFrameSet::Physics,
    );
    assert_system_in_stage(
        graph,
        resolve_camera_pose,
        "resolve_camera_pose",
        ClientFrameSet::Camera,
    );
    assert_system_in_stage(
        graph,
        publish_local_player_frame,
        "publish_local_player_frame",
        ClientFrameSet::Interaction,
    );
    assert_system_in_stage(
        graph,
        publish_interaction_origin,
        "publish_interaction_origin",
        ClientFrameSet::Interaction,
    );
    assert_system_in_stage(
        graph,
        drive_world_stream,
        "drive_world_stream",
        ClientFrameSet::WorldPublication,
    );
    assert_system_in_stage(
        graph,
        advance_actor_frame,
        "advance_actor_frame",
        ClientFrameSet::ActorPreparation,
    );
    assert_system_in_stage(
        graph,
        prepare_actor_render_frame,
        "prepare_actor_render_frame",
        ClientFrameSet::ActorFinalization,
    );
    assert_system_in_stage(
        graph,
        prepare_ui_runtime,
        "prepare_ui_runtime",
        ClientFrameSet::UiPreparation,
    );
    assert_system_in_stage(
        graph,
        publish_actor_render_frame,
        "publish_actor_render_frame",
        ClientFrameSet::ActorPublication,
    );
    assert_system_in_stage(
        graph,
        publish_ui_runtime,
        "publish_ui_runtime",
        ClientFrameSet::UiPublication,
    );
    assert_system_in_stage(
        graph,
        send_player_auth_inputs,
        "send_player_auth_inputs",
        ClientFrameSet::NetworkSend,
    );
    for preparation in [
        system_node(graph, drive_world_stream, "drive_world_stream"),
        system_node(graph, advance_actor_frame, "advance_actor_frame"),
        system_node(graph, prepare_ui_runtime, "prepare_ui_runtime"),
    ] {
        assert!(
            schedule_precedes(
                graph,
                system_node(graph, send_player_auth_inputs, "send_player_auth_inputs"),
                preparation,
            ),
            "gameplay packets must not wait for world, actor or UI preparation",
        );
    }
    assert!(
        schedule_precedes(
            graph,
            system_node(graph, emit_phase3_evidence, "emit_phase3_evidence"),
            system_node(graph, produce_melee, "produce_melee"),
        ),
        "the exact build/session/PREG/BREG identity marker must precede attack production",
    );
    assert!(
        schedule_precedes(
            graph,
            system_node(graph, produce_melee, "produce_melee"),
            system_node(graph, produce_survival_mining, "produce_survival_mining"),
        ),
        "an attacked actor must veto mining the block behind it",
    );
    assert!(
        schedule_precedes(
            graph,
            system_node(graph, produce_block_use, "produce_block_use"),
            system_node(graph, advance_local_physics, "advance_local_physics"),
        ) && schedule_precedes(
            graph,
            system_node(graph, produce_block_use, "produce_block_use"),
            system_node(graph, produce_item_use, "produce_item_use"),
        ),
        "build actions must resolve before the tick's movement and before air use",
    );
    assert!(
        schedule_precedes(
            graph,
            system_node(graph, produce_item_use, "produce_item_use"),
            system_node(graph, send_player_auth_inputs, "send_player_auth_inputs"),
        ),
        "air use must attach before the candidate packet is sent",
    );
    assert_system_in_stage(
        graph,
        produce_survival_mining,
        "produce_survival_mining",
        ClientFrameSet::NetworkSend,
    );
    assert_system_in_stage(
        graph,
        emit_phase3_evidence,
        "emit_phase3_evidence",
        ClientFrameSet::NetworkSend,
    );

    assert!(
        schedule_precedes(
            graph,
            system_node(
                graph,
                publish_local_player_frame,
                "publish_local_player_frame"
            ),
            system_node(
                graph,
                publish_interaction_origin,
                "publish_interaction_origin"
            ),
        ),
        "the atomic local-player frame must publish before its interaction consumer"
    );
    assert!(
        schedule_precedes(
            graph,
            system_node(graph, receive_network_events, "receive_network_events"),
            stage_node(graph, ClientFrameSet::Physics),
        ),
        "correction/session/dimension ingress must invalidate state before Physics and Interaction"
    );
    assert!(
        schedule_precedes(
            graph,
            system_node(
                graph,
                reconcile_world_stream_before_physics,
                "reconcile_world_stream_before_physics",
            ),
            stage_node(graph, ClientFrameSet::Physics),
        ),
        "committed correction and dimension reconciliation must finish before Physics",
    );
}

#[test]
fn acceptance_terminal_runs_after_the_authoritative_network_send_stage() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    configure_client_production_frame_systems(&mut app);
    configure_acceptance_finish_system(&mut app);

    let schedules = app.world().resource::<Schedules>();
    let graph = schedules
        .get(Update)
        .expect("production Update schedule")
        .graph();
    assert!(
        schedule_precedes(
            graph,
            stage_node(graph, ClientFrameSet::NetworkSend),
            system_node(graph, finish_acceptance_run, "finish_acceptance_run"),
        ),
        "terminal evidence must sample the final acknowledgement-drain state after NetworkSend closes admissions",
    );
    assert!(
        schedule_precedes(
            graph,
            stage_node(graph, ClientFrameSet::NetworkSend),
            system_node(graph, recover_session_failure, "recover_session_failure",),
        ),
        "launcher recovery must observe send-side failures from the same frame before fatal exit",
    );
}

/// Checks ordering inherited from containing sets without treating shared membership as order.
fn schedule_precedes(graph: &ScheduleGraph, from: NodeId, target: NodeId) -> bool {
    let ancestors = |node| {
        let mut pending = vec![node];
        let mut found = Vec::new();
        while let Some(node) = pending.pop() {
            if !found.contains(&node) {
                found.push(node);
                pending.extend(
                    graph
                        .hierarchy()
                        .graph()
                        .neighbors_directed(node, Direction::Incoming),
                );
            }
        }
        found
    };
    let targets = ancestors(target);
    ancestors(from).into_iter().any(|from| {
        targets.iter().any(|&target| {
            from != target && schedule_reaches(graph.dependency().graph(), from, target)
        })
    })
}

/// Follows explicit graph edges through any number of intermediate nodes.
fn schedule_reaches(graph: &DiGraph<NodeId>, from: NodeId, target: NodeId) -> bool {
    let mut pending = vec![from];
    let mut visited = Vec::new();
    while let Some(node) = pending.pop() {
        if node == target {
            return true;
        }
        if !visited.contains(&node) {
            visited.push(node);
            pending.extend(graph.neighbors(node));
        }
    }
    false
}

fn stage_node(graph: &ScheduleGraph, stage: ClientFrameSet) -> NodeId {
    let key = graph
        .system_sets
        .get_key(stage.intern())
        .unwrap_or_else(|| panic!("missing production stage {stage:?}"));
    NodeId::Set(key)
}

fn assert_system_in_stage<M>(
    graph: &ScheduleGraph,
    system: impl IntoSystemSet<M>,
    label: &str,
    stage: ClientFrameSet,
) {
    assert!(
        schedule_reaches(
            graph.hierarchy().graph(),
            stage_node(graph, stage),
            system_node(graph, system, label),
        ),
        "production system {label} is not a member of {stage:?}"
    );
}

fn system_node<M>(graph: &ScheduleGraph, system: impl IntoSystemSet<M>, label: &str) -> NodeId {
    let type_set = graph
        .system_sets
        .get_key(system.into_system_set().intern())
        .unwrap_or_else(|| panic!("missing production system type set {label}"));
    let parent = NodeId::Set(type_set);
    let mut matches = graph.systems.iter().filter_map(|(key, _, _)| {
        let child = NodeId::System(key);
        graph
            .hierarchy()
            .graph()
            .contains_edge(parent, child)
            .then_some(child)
    });
    let node = matches
        .next()
        .unwrap_or_else(|| panic!("missing production system {label}"));
    assert!(
        matches.next().is_none(),
        "production system {label} is registered more than once"
    );
    node
}

#[derive(bevy::prelude::Resource)]
struct UnifiedPublicationFixture {
    stream: chunk_pipeline::WorldStream,
    mesh: meshing::ChunkMesh,
    next_payload: usize,
    ingestion_frames: usize,
    expected: std::collections::BTreeMap<world::SubChunkKey, u64>,
    expected_known_air: std::collections::BTreeMap<world::SubChunkKey, u64>,
    acknowledged: std::collections::BTreeMap<world::SubChunkKey, u64>,
    acknowledged_known_air: std::collections::BTreeMap<world::SubChunkKey, u64>,
}

fn publication_fixture_key(index: usize) -> world::SubChunkKey {
    world::SubChunkKey::new(
        0,
        49 + (index % 33) as i32,
        (index / (33 * 33)) as i32,
        49 + ((index / 33) % 33) as i32,
    )
}

fn publication_fixture_mesh(runtime_assets: &assets::RuntimeAssets) -> meshing::ChunkMesh {
    let source = world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
    meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        runtime_assets,
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &source,
    )
}

fn drive_unified_publication_fixture(
    mut fixture: bevy::prelude::ResMut<UnifiedPublicationFixture>,
    mut controller: bevy::prelude::ResMut<PublicationController>,
    mut budget: bevy::prelude::ResMut<render::ChunkUploadBudget>,
    mut render_queue: bevy::prelude::ResMut<render::ChunkRenderQueue>,
    acknowledgements: bevy::prelude::Res<render::ChunkUploadAcknowledgements>,
) {
    const COHORT_ITEMS: usize = 6_951;
    const PAYLOADS_PER_FRAME: usize = 510;

    for acknowledgement in acknowledgements.drain() {
        let generation = acknowledgement.token.generation;
        if let Some(&expected_generation) = fixture.expected.get(&acknowledgement.key) {
            assert_eq!(generation, expected_generation);
            assert!(acknowledgement.uploaded_bytes > 0);
            assert!(
                fixture
                    .acknowledged
                    .insert(acknowledgement.key, generation)
                    .is_none(),
                "one payload produced a duplicate acknowledgement"
            );
        } else {
            assert_eq!(
                fixture.expected_known_air.get(&acknowledgement.key),
                Some(&generation)
            );
            assert_eq!(acknowledgement.uploaded_bytes, 0);
            assert!(
                fixture
                    .acknowledged_known_air
                    .insert(acknowledgement.key, generation)
                    .is_none(),
                "one known-air removal produced a duplicate acknowledgement"
            );
        }
        fixture.stream.acknowledge_mesh_upload(
            acknowledgement.key,
            generation,
            acknowledgement.token.dirty_since,
            acknowledgement.applied_at,
        );
    }

    controller.begin_frame(Duration::from_millis(125));
    *budget = controller.budget();
    fixture
        .stream
        .set_publication_allowance(controller.allowance());

    if fixture.next_payload < COHORT_ITEMS {
        let frame_number = fixture.ingestion_frames + 1;
        let frame_end = fixture
            .next_payload
            .saturating_add(PAYLOADS_PER_FRAME)
            .min(COHORT_ITEMS);
        let entries = (fixture.next_payload..frame_end)
            .map(|index| {
                (
                    publication_fixture_key(index),
                    fixture.mesh.clone(),
                    meshing::PackedBiomeRecord::fallback(),
                )
            })
            .collect();
        let identities = fixture
            .stream
            .stage_publication_fixture_completions(entries);
        for (index, identity) in (fixture.next_payload..frame_end).zip(identities) {
            let key = publication_fixture_key(index);
            let expected_generation = index as u64 + fixture.ingestion_frames as u64 + 1;
            assert_eq!(identity.key, key);
            assert_eq!(identity.generation, expected_generation);
            assert!(
                fixture.expected.insert(key, expected_generation).is_none(),
                "fixture keys are independently unique"
            );
        }
        fixture.next_payload = frame_end;

        let air_key = world::SubChunkKey::new(0, -20_000 - frame_number as i32, 0, 0);
        let expected_air_generation = frame_end as u64 + frame_number as u64;
        let identity = fixture.stream.stage_publication_fixture_known_air(air_key);
        assert_eq!(identity.key, air_key);
        assert_eq!(identity.generation, expected_air_generation);
        assert!(
            fixture
                .expected_known_air
                .insert(air_key, expected_air_generation)
                .is_none()
        );
        fixture.ingestion_frames = frame_number;
    }

    fixture.stream.service_publication_fixture_completions();
    let poll = fixture
        .stream
        .poll([1_048.0, 64.0, 1_048.0], budget.max_per_frame);
    let mut published_items = 0_usize;
    let mut published_payloads = 0_usize;
    let mut published_bytes = 0_u64;
    while let Some(change) = fixture.stream.pop_mesh_change() {
        match change {
            chunk_pipeline::WorldMeshChange::Upsert {
                output_permit: _,
                key,
                mesh,
                biome,
                tint_identity,
                generation,
                dirty_since,
                urgent: _,
                permit,
            } => {
                let bytes = render::ChunkRenderQueue::upload_byte_len(&mesh, &biome);
                render_queue
                    .try_update_tracked_with_biome_identity_permitted(
                        key,
                        mesh,
                        biome,
                        tint_identity,
                        render::ChunkUploadPriority::new(0.0),
                        render::ChunkUploadToken {
                            generation,
                            dirty_since,
                        },
                        permit.expect("real WorldStream admission attaches a payload permit"),
                    )
                    .unwrap();
                published_payloads += 1;
                published_bytes = published_bytes.saturating_add(bytes);
            }
            chunk_pipeline::WorldMeshChange::Remove {
                key,
                generation,
                dirty_since,
                urgent: _,
                permit,
            } => {
                render_queue
                    .try_remove_tracked_permitted(
                        key,
                        render::ChunkUploadPriority::new(f32::MAX),
                        render::ChunkUploadToken {
                            generation,
                            dirty_since,
                        },
                        permit.expect("real known-air admission attaches a zero-byte permit"),
                    )
                    .unwrap();
            }
        }
        published_items += 1;
    }
    controller.finish_frame(PublicationFrameWork {
        mesh_jobs_dispatched: poll.mesh_jobs_dispatched,
        mesh_changes_published: published_items,
        mesh_payloads_published: published_payloads,
        mesh_bytes_published: published_bytes,
        pending_mesh_jobs: fixture.stream.stats().pending_mesh_jobs,
        in_flight_mesh_jobs: fixture.stream.stats().in_flight_mesh_jobs,
        upload_queue_items: render_queue.retained_len(),
        upload_queue_bytes: render_queue.pending_bytes(),
        ..PublicationFrameWork::healthy()
    });
}

#[test]
fn production_pipeline_presents_exact_6951_manifest_with_known_air_within_sixteen_frames() {
    use std::{collections::BTreeMap, sync::Arc, time::Instant};

    use bevy::{
        asset::{AssetPlugin, Assets},
        camera::{
            Camera, Camera3d, CameraPlugin, OrthographicProjection, Projection, RenderTarget,
            ScalingMode,
        },
        core_pipeline::CorePipelinePlugin,
        image::{Image, ImagePlugin},
        mesh::MeshPlugin,
        prelude::{App, IntoScheduleConfigs, MinimalPlugins, Msaa, Transform, Update, Vec3},
        render::render_resource::TextureFormat,
        window::WindowPlugin,
    };
    use protocol::WorldBootstrap;
    use render::{
        ChunkBiomeTints, ChunkRenderApplySet, ChunkRenderPlugin, ChunkTextureAssets,
        ChunkUploadAcknowledgements, ChunkUploadBudget, PresentedFrameGate, RenderViewCohort,
        TargetRenderExpectation, publication_noop_render_plugin,
        publication_render_terminal_snapshot, settle_publication_noop_frame,
    };

    const COHORT_ITEMS: usize = 6_951;
    let config = PublicationServiceConfig::PHASE2_GATE;
    let runtime_assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let stream = chunk_pipeline::WorldStream::new_with_assets(
        WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [1_048.0, 64.0, 1_048.0],
            world_spawn_position: [1_048, 64, 1_048],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::clone(&runtime_assets),
        [1_048.0, 64.0, 1_048.0],
        None,
    );
    let tint_identity = stream.biome_tint_identity();
    let biome_tints = ChunkBiomeTints::from_resolved_with_identity(
        &stream.resolved_biome_tints_snapshot(),
        tint_identity,
    );
    let mesh = publication_fixture_mesh(&runtime_assets);
    assert!(!mesh.is_empty());

    let fixture = UnifiedPublicationFixture {
        stream,
        mesh,
        next_payload: 0,
        ingestion_frames: 0,
        expected: BTreeMap::new(),
        expected_known_air: BTreeMap::new(),
        acknowledged: BTreeMap::new(),
        acknowledged_known_air: BTreeMap::new(),
    };
    let initial_budget =
        ChunkUploadBudget::new(config.maximum_frame_items, config.maximum_frame_bytes)
            .with_zero_byte_operations_per_frame(config.maximum_zero_byte_operations_per_frame);

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..Default::default()
        })
        .add_plugins(AssetPlugin::default())
        .add_plugins(publication_noop_render_plugin())
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ))
        .insert_resource(ChunkTextureAssets::new(Arc::clone(&runtime_assets)))
        .insert_resource(biome_tints)
        .insert_resource(PublicationController::new(config))
        .insert_resource(fixture)
        .add_plugins(ChunkRenderPlugin::with_budget(initial_budget))
        .add_systems(
            Update,
            drive_unified_publication_fixture.before(ChunkRenderApplySet),
        );

    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            512,
            512,
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        ));
    app.world_mut().spawn((
        Camera3d::default(),
        Camera::default(),
        RenderTarget::Image(target.into()),
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: 800.0,
            },
            near: -2_500.0,
            far: 2_500.0,
            ..OrthographicProjection::default_3d()
        }),
        Msaa::Off,
        Transform::from_xyz(1_048.0, 2_000.0, 1_048.0)
            .looking_at(Vec3::new(1_048.0, 48.0, 1_048.0), Vec3::Z),
    ));
    app.finish();
    app.cleanup();

    let started = Instant::now();
    let mut completed_frames = 0_usize;
    while app
        .world()
        .resource::<UnifiedPublicationFixture>()
        .next_payload
        < COHORT_ITEMS
    {
        completed_frames += 1;
        assert!(completed_frames <= 14);
        settle_publication_noop_frame(&mut app);
    }
    assert_eq!(completed_frames, 14);
    assert!(completed_frames * 125 <= 2_000);

    let expected = app
        .world()
        .resource::<UnifiedPublicationFixture>()
        .expected
        .clone();
    assert_eq!(expected.len(), COHORT_ITEMS);
    let expectation = TargetRenderExpectation {
        cohort: RenderViewCohort::new(0, [65, 65], 16),
        source_cohort: None,
        target_columns: None,
        target_keys: Some(Arc::from(expected.keys().copied().collect::<Vec<_>>())),
        manifest: Arc::from(
            expected
                .iter()
                .map(|(&key, &generation)| (key, generation))
                .collect::<Vec<_>>(),
        ),
        view_generation: 1,
        render_ready_at: started,
    };
    let presented_gate = app.world().resource::<PresentedFrameGate>().clone();
    presented_gate.set_expectation(expectation);

    settle_publication_noop_frame(&mut app);
    completed_frames += 1;
    settle_publication_noop_frame(&mut app);
    completed_frames += 1;
    assert_eq!(completed_frames, 16);

    let fixture = app.world().resource::<UnifiedPublicationFixture>();
    assert_eq!(fixture.acknowledged, fixture.expected);
    assert_eq!(fixture.acknowledged_known_air, fixture.expected_known_air);
    assert_eq!(fixture.expected_known_air.len(), 14);
    assert_eq!(
        fixture.stream.publication_fixture_snapshot(),
        chunk_pipeline::PublicationFixtureSnapshot {
            pending_mesh_jobs: 0,
            in_flight_mesh_jobs: 0,
            pending_mesh_changes: 0,
            unacknowledged_meshes: 0,
        }
    );
    let controller = app.world().resource::<PublicationController>();
    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
    assert!(controller.accrued_items() <= config.maximum_burst_items);
    assert!(controller.accrued_bytes() <= config.maximum_burst_bytes);
    assert_eq!(controller.allowance().live_permits(), 0);
    assert!(
        app.world()
            .resource::<ChunkUploadAcknowledgements>()
            .is_empty()
    );
    assert_eq!(
        app.world()
            .resource::<render::ChunkRenderQueue>()
            .retained_len(),
        0
    );

    let terminal = publication_render_terminal_snapshot(&mut app);
    let expected_manifest = expected.into_iter().collect::<Vec<_>>();
    assert_eq!(terminal.extracted_manifest, expected_manifest);
    assert_eq!(terminal.allocation_manifest, expected_manifest);
    assert_eq!(terminal.pending_gpu_removals, 0);
    assert_eq!(terminal.fairness_waiters, 0);
    assert_eq!(terminal.retired_allocations, 0);
    assert_eq!(terminal.pending_arena_removals, 0);
    assert_eq!(terminal.in_flight_presented_callbacks, 0);
    assert!(!terminal.transparent_presentation_in_flight);
    assert!(!terminal.transparent_retirement_in_flight);

    let presented = presented_gate.drain();
    assert_eq!(presented.len(), 2);
    let pipeline_errors: Vec<_> = app
        .sub_app(bevy::render::RenderApp)
        .world()
        .resource::<bevy::render::render_resource::PipelineCache>()
        .pipelines()
        .filter_map(|pipeline| match &pipeline.state {
            bevy::render::render_resource::CachedPipelineState::Err(error) => {
                Some(error.to_string())
            }
            _ => None,
        })
        .collect();
    assert!(pipeline_errors.is_empty(), "{pipeline_errors:?}");
    assert!(
        presented[0].is_exact(),
        "allocations={}, visible={}, drawn={}, missing={}, unexpected={}, source={}, foreign={}, stale={}, orphans={}",
        presented[0].allocation_manifest.len(),
        presented[0].visible_allocation_manifest.len(),
        presented[0].drawn_manifest.len(),
        presented[0].missing_target_instances,
        presented[0].unexpected_target_instances,
        presented[0].source_instances,
        presented[0].foreign_instances,
        presented[0].stale_generation_instances,
        presented[0].orphan_allocations,
    );
    assert!(presented[0].forms_stable_exact_pair_with(&presented[1]));
}

#[test]
fn permitted_removal_crosses_real_extraction_and_physically_frees_an_existing_gpu_allocation_once()
{
    use bevy::{
        asset::AssetPlugin,
        camera::CameraPlugin,
        core_pipeline::CorePipelinePlugin,
        image::ImagePlugin,
        mesh::MeshPlugin,
        prelude::{App, MinimalPlugins},
        window::WindowPlugin,
    };
    use meshing::{ChunkBiomeTintIdentity, PackedBiomeRecord};
    use render::{
        ChunkRenderPlugin, ChunkRenderQueue, ChunkUploadAcknowledgements, ChunkUploadBudget,
        ChunkUploadPriority, ChunkUploadToken, publication_noop_render_plugin,
        publication_render_terminal_snapshot, settle_publication_noop_frame,
    };
    use world::SubChunkKey;

    let config = PublicationServiceConfig::PHASE2_GATE;
    assert_eq!(config.maximum_zero_byte_operations_per_frame, 256);
    let runtime_assets = assets::RuntimeAssets::diagnostic();
    let mesh = publication_fixture_mesh(&runtime_assets);
    let biome = PackedBiomeRecord::fallback();
    let bytes = ChunkRenderQueue::upload_byte_len(&mesh, &biome);
    let allowance = chunk_pipeline::PublicationAllowance::new(config);
    allowance.begin_frame(
        1,
        1,
        config.maximum_frame_bytes,
        0,
        config.maximum_frame_items,
    );
    let upload_permit = allowance.try_admit_payload(bytes).unwrap();
    let acknowledgements = ChunkUploadAcknowledgements::default();
    let initial_budget =
        ChunkUploadBudget::new(config.maximum_frame_items, config.maximum_frame_bytes)
            .with_zero_byte_operations_per_frame(config.maximum_zero_byte_operations_per_frame);

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..Default::default()
        })
        .add_plugins(AssetPlugin::default())
        .add_plugins(publication_noop_render_plugin())
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ))
        .insert_resource(acknowledgements.clone())
        .add_plugins(ChunkRenderPlugin::with_budget(initial_budget));
    app.finish();
    app.cleanup();

    let key = SubChunkKey::new(0, 9, 0, 9);
    let now = std::time::Instant::now();
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked_with_biome_identity_permitted(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::default(),
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 1,
                dirty_since: now,
            },
            upload_permit,
        )
        .unwrap();
    settle_publication_noop_frame(&mut app);

    let uploaded = acknowledgements.drain();
    assert_eq!(uploaded.len(), 1);
    assert_eq!(uploaded[0].key, key);
    assert_eq!(uploaded[0].token.generation, 1);
    assert!(uploaded[0].uploaded_bytes >= bytes);
    let uploaded_terminal = publication_render_terminal_snapshot(&mut app);
    assert_eq!(uploaded_terminal.extracted_manifest, vec![(key, 1)]);
    assert_eq!(uploaded_terminal.allocation_manifest, vec![(key, 1)]);
    assert_eq!(allowance.live_permits(), 0);

    allowance.begin_frame(2, 1, 0, 1, config.maximum_frame_items);
    let removal_permit = allowance.try_admit_zero_byte().unwrap();
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_remove_tracked_permitted(
            key,
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 2,
                dirty_since: now,
            },
            removal_permit,
        )
        .unwrap();
    settle_publication_noop_frame(&mut app);

    let removed = acknowledgements.drain();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].key, key);
    assert_eq!(removed[0].token.generation, 2);
    assert_eq!(removed[0].uploaded_bytes, 0);
    let removed_terminal = publication_render_terminal_snapshot(&mut app);
    assert!(removed_terminal.extracted_manifest.is_empty());
    assert!(removed_terminal.allocation_manifest.is_empty());
    assert_eq!(removed_terminal.pending_gpu_removals, 0);
    assert_eq!(removed_terminal.pending_arena_removals, 0);
    assert_eq!(allowance.live_permits(), 0);

    settle_publication_noop_frame(&mut app);
    assert!(acknowledgements.is_empty());
    assert_eq!(allowance.live_permits(), 0);
}

#[test]
fn per_frame_work_distinguishes_backlog_from_visibility_loss() {
    let mut controller = PublicationController::default();
    controller.begin_frame(Duration::from_millis(10));
    controller.finish_frame(PublicationFrameWork {
        mesh_jobs_dispatched: 7,
        mesh_changes_published: 5,
        mesh_payloads_published: 5,
        mesh_bytes_published: 900_000,
        pending_mesh_jobs: 123,
        in_flight_mesh_jobs: 11,
        upload_queue_items: 17,
        upload_queue_bytes: 2_000_000,
        allowance_live_permits: 5,
        allowance_live_payload_bytes: 900_000,
        stream_pending_mesh_changes: 3,
        cohort_expected: 1_089,
        cohort_loaded: 900,
        resident_meshes: 850,
        cave_visible_meshes: 700,
        frustum_visible_meshes: 410,
        submitted_meshes: 410,
        gpu_completed_meshes: 410,
    });

    let diagnostics = controller.diagnostics();
    assert_eq!(diagnostics.last_work.pending_mesh_jobs, 123);
    assert_eq!(diagnostics.last_work.frustum_visible_meshes, 410);
    assert_eq!(diagnostics.last_work.submitted_meshes, 410);
    assert_eq!(diagnostics.last_work.gpu_completed_meshes, 410);
}

#[test]
fn adaptive_publication_diagnostic_is_deterministic_and_cohort_tagged() {
    let mut controller = PublicationController::default();
    controller.begin_frame(Duration::from_millis(10));
    controller.finish_frame(PublicationFrameWork {
        mesh_jobs_dispatched: 7,
        mesh_changes_published: 5,
        mesh_payloads_published: 5,
        mesh_bytes_published: 900_000,
        pending_mesh_jobs: 123,
        in_flight_mesh_jobs: 11,
        upload_queue_items: 17,
        upload_queue_bytes: 2_000_000,
        allowance_live_permits: 5,
        allowance_live_payload_bytes: 900_000,
        stream_pending_mesh_changes: 3,
        cohort_expected: 1_089,
        cohort_loaded: 900,
        resident_meshes: 850,
        cave_visible_meshes: 700,
        frustum_visible_meshes: 410,
        submitted_meshes: 410,
        gpu_completed_meshes: 410,
    });

    let line = adaptive_publication_diagnostic_line(controller.diagnostics());
    assert_eq!(
        line,
        "ADAPTIVE_PUBLICATION frame=1 frame_us=10000 cap_items=81 cap_bytes=1342177 cap_zero=256 under_target_streak=0 decreases=0 increases=0 dispatched=7 published=5 published_payload_items=5 published_zero_items=0 published_bytes=900000 pending=123 in_flight=11 upload_items=17 upload_bytes=2000000 live_permits=5 live_bytes=900000 stream_changes=3 cohort_loaded=900 cohort_expected=1089 resident=850 cave=700 frustum=410 submitted=410 gpu_completed=410"
    );
}

#[test]
fn production_world_handoff_fails_closed_without_a_linear_publication_permit() {
    let missing = WorldMeshChange::Remove {
        key: world::SubChunkKey::new(0, 0, 0, 0),
        generation: 1,
        dirty_since: std::time::Instant::now(),
        urgent: false,
        permit: None,
    };
    assert!(!mesh_change_has_publication_permit(&missing));

    let source = include_str!("../runtime/world.rs");
    assert!(source.contains("MISSING_PUBLICATION_PERMIT_ERROR"));
    assert!(!source.contains("render_queue.try_update_tracked_with_biome_identity("));
    assert!(!source.contains("render_queue.try_remove_tracked("));
    assert!(source.contains("try_update_tracked_with_biome_identity_permitted("));
    assert!(source.contains("try_remove_tracked_permitted("));
}

#[test]
fn local_player_pipeline_orders_physics_camera_and_interaction_and_has_one_camera_writer() {
    let app_source = include_str!("../app.rs");
    assert!(app_source.contains("LocalPlayerFrameSet::Physics"));
    assert!(app_source.contains("LocalPlayerFrameSet::Camera"));
    assert!(app_source.contains("LocalPlayerFrameSet::Interaction"));

    let production_sources = [
        include_str!("../camera.rs"),
        include_str!("../movement.rs"),
        include_str!("../runtime/network.rs"),
        include_str!("../runtime/world.rs"),
        include_str!("../local_player.rs"),
    ];
    assert_eq!(
        production_sources
            .iter()
            .map(|source| source.matches("&mut Transform").count())
            .sum::<usize>(),
        1,
        "only the CameraPose publication adapter may borrow the base camera Transform"
    );

    let local_player = include_str!("../local_player.rs");
    let adapter = local_player
        .split_once("pub(crate) fn resolve_camera_pose")
        .expect("camera resolver adapter exists")
        .1
        .split_once("pub(crate) fn publish_local_player_frame")
        .expect("camera resolver adapter has a bounded body")
        .0;
    assert_eq!(
        adapter
            .matches("client_presentation::local_player::resolve_camera_pose(")
            .count(),
        1,
        "the production adapter must call the presentation camera writer once"
    );
    assert!(!adapter.contains("*camera_transform"));

    let presentation = include_str!("../../../crates/client-presentation/src/local_player.rs");
    assert_eq!(presentation.matches("&mut Transform").count(), 1);
    let resolver = presentation
        .split_once("pub fn resolve_camera_pose")
        .expect("presentation camera resolver exists")
        .1
        .split_once("pub fn publish_local_player_frame")
        .expect("presentation camera resolver has a bounded body")
        .0;
    assert_eq!(
        resolver.matches("*camera_transform.1 = transform;").count(),
        1
    );
    assert!(
        resolver.contains("collision_safe_perspective_pose("),
        "the base camera writer must use the swept collision solver"
    );
}

#[test]
fn production_stage_capacities_can_carry_one_literal_maximum_payload_frame() {
    let config = PublicationServiceConfig::PHASE2_GATE;

    assert!(chunk_pipeline::WORK_RESULT_CAPACITY >= config.maximum_frame_items);
    assert!(chunk_pipeline::MAX_PENDING_MESH_CHANGES >= config.maximum_frame_items);
    assert!(render::ChunkRenderQueueLimits::default().max_items >= config.maximum_frame_items);
}

#[test]
fn application_wires_controller_before_world_handoff_and_render_apply() {
    // The graph test above covers execution order. Startup still must install
    // the gate controller; constructing fixtures alone cannot prove that wiring.
    let source = include_str!("../app.rs");
    assert!(source.contains("PublicationController::new("));
    assert!(source.contains("PublicationServiceConfig::PHASE2_GATE"));
}
