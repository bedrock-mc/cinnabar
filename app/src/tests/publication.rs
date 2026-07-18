use std::time::Duration;

use crate::runtime::publication::{
    PublicationController, PublicationFrameWork, adaptive_publication_diagnostic_line,
};
use crate::runtime::world::mesh_change_has_publication_permit;
use client_world::{PublicationServiceConfig, WorldMeshChange};

#[derive(bevy::prelude::Resource)]
struct UnifiedPublicationFixture {
    stream: client_world::WorldStream,
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
    let source = world::SubChunk::decode(&[9, 1, 0, 1, 2])
        .expect("decode deterministic solid publication source");
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

    let poll = fixture
        .stream
        .poll([1_048.0, 64.0, 1_048.0], budget.max_per_frame);
    let mut published_items = 0_usize;
    let mut published_payloads = 0_usize;
    let mut published_bytes = 0_u64;
    while let Some(change) = fixture.stream.pop_mesh_change() {
        match change {
            client_world::WorldMeshChange::Upsert {
                key,
                mesh,
                biome,
                tint_identity,
                generation,
                dirty_since,
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
            client_world::WorldMeshChange::Remove {
                key,
                generation,
                dirty_since,
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
    let stream = client_world::WorldStream::new_with_assets(
        WorldBootstrap {
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
        client_world::PublicationFixtureSnapshot {
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
    assert!(presented[0].is_exact());
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
    let allowance = client_world::PublicationAllowance::new(config);
    allowance.begin_frame(1, 1, config.maximum_frame_bytes, 0);
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

    allowance.begin_frame(2, 0, 0, 1);
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
        "ADAPTIVE_PUBLICATION frame=1 frame_us=10000 cap_items=81 cap_bytes=1342177 cap_zero=256 under_target_streak=0 decreases=0 increases=0 dispatched=7 published=5 published_bytes=900000 pending=123 in_flight=11 upload_items=17 upload_bytes=2000000 cohort_loaded=900 cohort_expected=1089 resident=850 cave=700 frustum=410 submitted=410 gpu_completed=410"
    );
}

#[test]
fn application_wires_controller_before_world_handoff_and_render_apply() {
    let source = include_str!("../app.rs");

    assert!(source.contains("PublicationController::new("));
    assert!(source.contains("PublicationServiceConfig::PHASE2_GATE"));
    let publication_frame = source[source
        .rfind("begin_publication_frame")
        .expect("publication frame system is registered")..]
        .split_once(".add_systems(")
        .expect("publication frame registration is bounded")
        .0;
    assert!(publication_frame.contains(".before(ChunkRenderApplySet)"));
    assert!(source.contains("drive_world_stream.before(ChunkRenderApplySet)"));
}

#[test]
fn production_world_handoff_fails_closed_without_a_linear_publication_permit() {
    let missing = WorldMeshChange::Remove {
        key: world::SubChunkKey::new(0, 0, 0, 0),
        generation: 1,
        dirty_since: std::time::Instant::now(),
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
fn fifo_jitter_accrues_wall_clock_service_without_frame_count_bias() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    let jitter = [
        Duration::from_micros(15_800),
        Duration::from_micros(16_667),
        Duration::from_micros(17_900),
        Duration::from_micros(16_200),
    ];
    let mut serviced = 0_usize;

    for frame in 0..240 {
        controller.begin_frame(jitter[frame % jitter.len()]);
        let allowance = controller.allowance();
        while let Some(permit) = allowance.try_admit_payload(1) {
            serviced = serviced.saturating_add(1);
            assert!(permit.retire());
        }
        controller.finish_frame(PublicationFrameWork::default());
    }

    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
    let elapsed_nanos = jitter
        .iter()
        .cycle()
        .take(240)
        .map(Duration::as_nanos)
        .sum::<u128>();
    let minimum = u128::from(config.minimum_items_per_second)
        .checked_mul(elapsed_nanos)
        .unwrap()
        / 1_000_000_000;
    assert!(u128::try_from(serviced).unwrap() >= minimum);
}

#[test]
fn one_eighty_millisecond_saturated_frame_reduces_to_the_literal_minimum_rate() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    controller.begin_frame(Duration::from_millis(16));
    let allowance = controller.allowance();
    while let Some(permit) = allowance.try_admit_payload(1) {
        assert!(permit.retire());
    }
    let carried_bytes = allowance.remaining_bytes();
    controller.finish_frame(PublicationFrameWork {
        pending_mesh_jobs: 1,
        mesh_changes_published: controller.budget().max_per_frame,
        mesh_payloads_published: controller.budget().max_per_frame,
        ..PublicationFrameWork::default()
    });

    controller.begin_frame(Duration::from_millis(80));

    let expected_items = usize::try_from(
        u64::from(config.minimum_items_per_second)
            .checked_mul(80)
            .unwrap()
            / 1_000,
    )
    .unwrap();
    let expected_bytes = carried_bytes
        .checked_add(config.minimum_bytes_per_second.checked_mul(80).unwrap() / 1_000)
        .unwrap();
    assert_eq!(controller.budget().max_per_frame, expected_items);
    assert_eq!(controller.budget().max_bytes_per_frame, expected_bytes);
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);
}

#[test]
fn zero_byte_removals_never_masquerade_as_spent_payload_authority() {
    let mut controller = PublicationController::default();
    controller.begin_frame(Duration::from_millis(16));
    controller.finish_frame(PublicationFrameWork {
        mesh_changes_published: PublicationServiceConfig::PHASE2_GATE
            .maximum_zero_byte_operations_per_frame,
        mesh_payloads_published: 0,
        mesh_bytes_published: 0,
        pending_mesh_jobs: 1,
        ..PublicationFrameWork::healthy()
    });

    controller.begin_frame(Duration::from_millis(80));

    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
}

#[test]
fn gpu_backlog_is_genuine_pressure_even_when_fifo_frame_time_is_healthy() {
    let mut controller = PublicationController::default();
    controller.finish_frame(PublicationFrameWork {
        upload_queue_items: client_world::MAX_PENDING_MESH_CHANGES,
        ..PublicationFrameWork::default()
    });

    controller.begin_frame(Duration::from_millis(16));

    assert!(controller.budget().max_per_frame <= 66);
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);
}

#[test]
fn pressure_recovers_only_after_healthy_frames_without_self_funded_bursts() {
    let mut controller = PublicationController::default();
    controller.finish_frame(PublicationFrameWork {
        upload_queue_items: client_world::MAX_PENDING_MESH_CHANGES,
        ..PublicationFrameWork::default()
    });
    controller.begin_frame(Duration::from_millis(16));
    let reduced = controller.budget().max_per_frame;
    let allowance = controller.allowance();
    while let Some(permit) = allowance.try_admit_payload(1) {
        assert!(permit.retire());
    }

    for _ in 0..119 {
        controller.finish_frame(PublicationFrameWork::default());
        controller.begin_frame(Duration::from_millis(16));
        assert!(controller.budget().max_per_frame <= reduced + 1);
        while let Some(permit) = allowance.try_admit_payload(1) {
            assert!(permit.retire());
        }
    }

    controller.finish_frame(PublicationFrameWork::default());
    controller.begin_frame(Duration::from_millis(16));
    assert!(controller.budget().max_per_frame >= 131);
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);
    assert_eq!(controller.diagnostics().additive_increases, 1);
}

#[test]
fn byte_tokens_follow_elapsed_time_and_never_cross_frame_or_burst_ceilings() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();

    controller.begin_frame(Duration::from_millis(125));
    assert_eq!(controller.budget().max_bytes_per_frame, 16 * 1024 * 1024);
    assert!(controller.budget().max_bytes_per_frame <= config.maximum_frame_bytes);

    controller.finish_frame(PublicationFrameWork::default());
    controller.begin_frame(Duration::MAX);
    assert!(controller.budget().max_per_frame <= config.maximum_frame_items);
    assert!(controller.budget().max_bytes_per_frame <= config.maximum_frame_bytes);
    assert!(controller.accrued_items() <= config.maximum_burst_items);
    assert!(controller.accrued_bytes() <= config.maximum_burst_bytes);
}

#[test]
fn idle_wall_time_never_accumulates_more_than_the_one_second_burst_ceiling() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();

    controller.begin_frame(Duration::from_secs(10));
    assert_eq!(controller.accrued_items(), config.maximum_burst_items);
    assert_eq!(controller.accrued_bytes(), config.maximum_burst_bytes);
    for _ in 0..=config.maximum_burst_items {
        controller.begin_frame(Duration::ZERO);
    }

    assert_eq!(controller.accrued_items(), config.maximum_burst_items);
    assert_eq!(controller.accrued_bytes(), config.maximum_burst_bytes);
}

#[test]
fn eight_hz_frames_receive_two_seconds_of_bounded_service_without_runaway_burst() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    let mut serviced = 0_usize;

    for _ in 0..16 {
        controller.begin_frame(Duration::from_millis(125));
        let allowance = controller.allowance();
        while let Some(permit) = allowance.try_admit_payload(1) {
            serviced = serviced.saturating_add(1);
            assert!(permit.retire());
        }
        controller.finish_frame(PublicationFrameWork::default());
    }

    assert!(
        serviced >= 6_951,
        "two wall-clock seconds at 8 Hz serviced only {serviced} items"
    );
    assert!(controller.budget().max_per_frame <= config.maximum_frame_items);
    assert!(controller.accrued_items() <= config.maximum_burst_items);
}

#[test]
fn paced_eight_hz_backlog_is_not_misclassified_as_publication_pressure() {
    let mut controller = PublicationController::default();
    let mut serviced = 0_usize;

    for _ in 0..16 {
        controller.begin_frame(Duration::from_millis(125));
        let budget = controller.budget();
        let allowance = controller.allowance();
        let mut published = 0;
        while let Some(permit) = allowance.try_admit_payload(1) {
            serviced = serviced.saturating_add(1);
            published += 1;
            assert!(permit.retire());
        }
        controller.finish_frame(PublicationFrameWork {
            mesh_changes_published: published,
            mesh_payloads_published: published,
            mesh_bytes_published: published as u64,
            pending_mesh_jobs: 5_461,
            in_flight_mesh_jobs: 32,
            upload_queue_items: 128,
            upload_queue_bytes: 32 * 1024 * 1024,
            ..PublicationFrameWork::healthy()
        });
        assert_eq!(published, budget.max_per_frame);
    }

    assert!(serviced >= 6_951);
    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
}

#[test]
fn publication_frame_is_explicitly_ordered_before_world_poll_and_handoff() {
    let source = include_str!("../app.rs");
    let publication = source
        .rfind("begin_publication_frame")
        .expect("publication frame system is registered");
    let registration = &source[publication..];

    assert!(registration.contains(".before(receive_network_events)"));
    assert!(registration.contains(".before(drive_world_stream)"));
}

#[test]
fn controller_credits_shared_allowance_and_only_admitted_work_spends_it() {
    let mut controller = PublicationController::default();
    let allowance = controller.allowance();

    controller.begin_frame(Duration::from_millis(125));
    let first_available = allowance.remaining_items();
    let permit = allowance.try_admit_payload(1).unwrap();
    assert!(permit.retire());
    controller.finish_frame(PublicationFrameWork::healthy());
    controller.begin_frame(Duration::from_millis(125));

    assert_eq!(first_available, 1_024);
    assert_eq!(allowance.remaining_items(), 2_047);
    assert_eq!(allowance.frame_remaining_items(), 512);
}

#[test]
fn production_stage_capacities_can_carry_one_literal_maximum_payload_frame() {
    let config = PublicationServiceConfig::PHASE2_GATE;

    assert!(client_world::WORK_RESULT_CAPACITY >= config.maximum_frame_items);
    assert!(client_world::MAX_PENDING_MESH_CHANGES >= config.maximum_frame_items);
    assert!(render::ChunkRenderQueueLimits::default().max_items >= config.maximum_frame_items);
}
