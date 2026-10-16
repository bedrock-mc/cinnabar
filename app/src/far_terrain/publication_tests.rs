//! Outgoing masks follow the real bounded near-mesh publication path.

use super::*;
use bevy::{
    asset::AssetPlugin,
    camera::CameraPlugin,
    core_pipeline::CorePipelinePlugin,
    image::ImagePlugin,
    mesh::MeshPlugin,
    prelude::{App, MinimalPlugins},
    render::RenderApp,
    window::WindowPlugin,
};
use chunk_pipeline::WorldMeshChange;
use far_terrain::{COLUMN_SIDE, TileKey, TileMesh, config::TILE_CELLS};
use render::{
    ChunkRenderPlugin, FarTerrainRenderPlugin, publication_noop_render_plugin,
    publication_render_terminal_snapshot, publication_terrain_draws, settle_publication_noop_frame,
};
use render_api::{PublicationAllowance, PublicationServiceConfig};
use render_scene::{
    ChunkBiomeTints, ChunkRenderQueue, ChunkTextureAssets, ChunkUploadAcknowledgements,
    ChunkUploadBudget, ChunkUploadPriority, ChunkUploadToken,
};
use world::{ChunkKey, SubChunkKey};

/// Transfers admitted world changes through the same permitted render queue used at runtime.
fn publish_changes(app: &mut App, stream: &mut WorldStream) {
    let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
    while let Some(change) = stream.pop_mesh_change() {
        match change {
            WorldMeshChange::Upsert {
                key,
                mesh,
                biome,
                tint_identity,
                generation,
                dirty_since,
                permit,
                ..
            } => queue
                .try_update_tracked_with_biome_identity_permitted(
                    key,
                    mesh,
                    biome,
                    tint_identity,
                    ChunkUploadPriority::new(0.0),
                    ChunkUploadToken {
                        generation,
                        dirty_since,
                    },
                    permit.expect("world admission grants an upload permit"),
                )
                .unwrap(),
            WorldMeshChange::Remove {
                key,
                generation,
                dirty_since,
                permit,
                ..
            } => queue
                .try_remove_tracked_permitted(
                    key,
                    ChunkUploadPriority::new(0.0),
                    ChunkUploadToken {
                        generation,
                        dirty_since,
                    },
                    stream.terrain_removal_kind(key),
                    permit.expect("world admission grants a removal permit"),
                )
                .unwrap(),
        }
    }
}

/// Applies only acknowledgements emitted by the real render schedule.
fn acknowledge(app: &App, stream: &mut WorldStream) -> usize {
    let receipts = app
        .world()
        .resource::<ChunkUploadAcknowledgements>()
        .drain();
    for receipt in &receipts {
        stream.acknowledge_mesh_upload(
            receipt.key,
            receipt.token.generation,
            receipt.token.dirty_since,
            receipt.applied_at,
        );
    }
    receipts.len()
}

/// Rebuilds and publishes the production near mask after stream acknowledgements.
fn refresh_mask(app: &mut App, far: &mut FarTerrain, stream: &WorldStream) {
    far.update_near(stream, ChunkKey::new(0, 0, 0), 1, 32);
    let mut scene = app.world_mut().resource_mut::<FarTerrainScene>();
    scene.near = Arc::clone(&far.near);
    far.publish_target(stream, 32, &mut scene);
}

/// Observes actual extracted near geometry and allocations, rather than handoff membership.
fn assert_near_geometry(app: &mut App, count: usize) {
    let state = publication_render_terminal_snapshot(app);
    assert_eq!(state.extracted_manifest.len(), count);
    assert_eq!(state.allocation_manifest.len(), count);
}

/// Requires one admitted draw route at every height, including physically retained sections.
fn assert_column_coverage(app: &mut App, column: ChunkKey, near_owned: bool) {
    let (near, far) = publication_terrain_draws(app, column);
    for y in 0..3 {
        let near_draw = near.contains(&SubChunkKey::from_chunk(column, y));
        assert_eq!(
            usize::from(near_draw) + usize::from(far),
            1,
            "exactly one drawer at section {y}"
        );
        assert_eq!(near_draw, near_owned);
    }
}

/// Builds the real extraction and render schedules with deterministic GPU resources.
fn publication_app(
    texture_assets: ChunkTextureAssets,
    tints: ChunkBiomeTints,
    budget: ChunkUploadBudget,
    scene: FarTerrainScene,
) -> App {
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
        .insert_resource(texture_assets)
        .insert_resource(tints)
        .insert_resource(scene)
        .add_plugins(ChunkRenderPlugin::with_budget(budget))
        .add_plugins(FarTerrainRenderPlugin);
    app.finish();
    app.cleanup();
    app
}

#[test]
fn publication_source_tracks_stream_and_dimension_visits_without_following_feature_toggles() {
    let bootstrap = protocol::WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    };
    let mut stream = WorldStream::new(bootstrap);
    let key = SubChunkKey::new(0, 0, 0, 0);
    stream.stage_publication_fixture_known_air(key);
    let mut far = FarTerrain::new(None);
    let mut scene = FarTerrainScene::default();
    far.publish_source(Some(&stream), &mut scene);
    let original = scene.publications.clone().unwrap();
    original.record(key, 1, true);
    for enabled in [true, false, true] {
        stream.set_far_terrain_enabled(enabled);
        far.publish_source(Some(&stream), &mut scene);
        assert!(scene.publications.as_ref().unwrap().same_source(&original));
        assert_eq!(
            scene
                .publications
                .as_ref()
                .unwrap()
                .changed_empty(None)
                .unwrap()
                .1[&key],
            1
        );
    }
    stream
        .submit(
            1,
            protocol::WorldEvent::ChangeDimension(protocol::ChangeDimensionEvent {
                dimension: 1,
                position: [0.0; 3],
                ..Default::default()
            }),
        )
        .unwrap();
    test_time::eventually("the publication dimension visit", || {
        stream.poll([0.0; 3], 0);
        stream.current_dimension() == 1
    });
    far.publish_source(Some(&stream), &mut scene);
    let dimension = scene.publications.clone().unwrap();
    assert!(!dimension.same_source(&original));
    assert!(dimension.changed_empty(None).unwrap().1.is_empty());
    let replacement = WorldStream::new(bootstrap);
    far.publish_source(Some(&replacement), &mut scene);
    assert!(!scene.publications.as_ref().unwrap().same_source(&dimension));
    far.publish_source(None, &mut scene);
    assert!(scene.publications.is_none());
}

#[test]
fn enabling_far_terrain_reuses_full_and_empty_gpu_publications_across_toggle_cycles() {
    let config = PublicationServiceConfig::DEFAULT;
    let allowance = PublicationAllowance::new(config);
    allowance.begin_frame(1, 2, 0, 2, config.maximum_frame_items);
    let column = ChunkKey::new(0, 5, 0);
    let solid = SubChunkKey::from_chunk(column, 0);
    let empty = SubChunkKey::from_chunk(column, 1);
    let air_column = ChunkKey::new(0, 9, 0);
    let air = SubChunkKey::from_chunk(air_column, 0);
    let publications = render_scene::terrain_publication::TerrainPublications::default();
    publications.retain([solid, empty, air].into_iter());
    let assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let source = world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
    let mesh = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &assets,
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &source,
    );
    assert!(!mesh.is_empty());
    let mut app = publication_app(
        ChunkTextureAssets::new(assets),
        ChunkBiomeTints::default(),
        ChunkUploadBudget::default(),
        FarTerrainScene {
            publications: Some(publications.clone()),
            ..Default::default()
        },
    );
    let token = ChunkUploadToken {
        generation: 1,
        dirty_since: Instant::now(),
    };
    {
        let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
        queue
            .try_update_tracked(solid, mesh, ChunkUploadPriority::new(0.0), token)
            .unwrap();
        for key in [empty, air] {
            queue
                .try_remove_tracked_permitted(
                    key,
                    ChunkUploadPriority::new(0.0),
                    token,
                    render_api::TerrainRemovalKind::Empty,
                    allowance.try_admit_zero_byte().unwrap(),
                )
                .unwrap();
        }
    }
    settle_publication_noop_frame(&mut app);
    assert_eq!(
        app.world()
            .resource::<ChunkUploadAcknowledgements>()
            .drain()
            .len(),
        3
    );
    assert_eq!(publications.changed_empty(None).unwrap().1.len(), 2);
    let before = publication_render_terminal_snapshot(&mut app).allocation_manifest;
    assert_eq!(before, [(solid, 1)]);
    let target = Arc::new(far_terrain::TerrainTarget {
        centre: column,
        horizon: 32,
        near: std::sync::Arc::new(std::collections::HashMap::from([
            (
                column,
                std::collections::HashMap::from([(solid, 1), (empty, 1)]),
            ),
            (air_column, std::collections::HashMap::from([(air, 1)])),
        ])),
        retained: std::sync::Arc::new([solid, empty, air].into_iter().collect()),
        empty_far: std::sync::Arc::new(Default::default()),
    });
    let tiles: Arc<[Arc<TileMesh>]> = [column, air_column]
        .map(|column| {
            Arc::new(TileMesh {
                coverage: far_terrain::TileCoverage::Complete,
                key: TileKey::containing(
                    0,
                    column.x * COLUMN_SIDE as i32,
                    column.z * COLUMN_SIDE as i32,
                ),
                revision: 1,
                quads: vec![[1 << 14 | 1 << 21, 64]].into_boxed_slice(),
                colours: vec![100; TILE_CELLS * TILE_CELLS * 4].into_boxed_slice(),
                min_y: 0,
                max_y: 64,
            })
        })
        .into();
    for _ in 0..2 {
        {
            let mut scene = app.world_mut().resource_mut::<FarTerrainScene>();
            scene.tiles = tiles.clone();
            scene.target = Some(target.clone());
        }
        settle_publication_noop_frame(&mut app);
        assert_eq!(
            publication_terrain_draws(&mut app, column),
            (vec![solid], false)
        );
        assert_eq!(
            publication_terrain_draws(&mut app, air_column),
            (vec![], false)
        );
        let actual = publication_render_terminal_snapshot(&mut app);
        assert_eq!(actual.allocation_manifest, before);
        assert_eq!((actual.near_updates, actual.near_upload_bytes), (0, 0));
        assert!(
            app.world()
                .resource::<ChunkUploadAcknowledgements>()
                .is_empty()
        );
        // Runtime disabling replaces the far residency, while preserving the near source.
        *app.world_mut().resource_mut::<FarTerrainScene>() = FarTerrainScene {
            publications: Some(publications.clone()),
            ..Default::default()
        };
        settle_publication_noop_frame(&mut app);
    }
    assert_eq!(allowance.live_permits(), 0);
}

#[test]
fn outgoing_draw_switch_is_atomic_through_delayed_and_budget_split_physical_removals() {
    let config = PublicationServiceConfig::DEFAULT;
    let allowance = PublicationAllowance::new(config);
    allowance.begin_frame(
        1,
        8,
        config.maximum_frame_bytes,
        0,
        config.maximum_frame_items,
    );
    let assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let mut stream = WorldStream::new_with_assets(
        protocol::WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [8.0, 64.0, 8.0],
            world_spawn_position: [0, 64, 0],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        },
        Arc::clone(&assets),
        [8.0, 64.0, 8.0],
        None,
    );
    stream.set_far_terrain_enabled(true);
    stream.set_publication_allowance(allowance.clone());
    let source = world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
    let mesh = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &assets,
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &source,
    );
    assert!(!mesh.is_empty());
    let column = ChunkKey::new(0, 5, 0);
    stream.stage_publication_fixture_completions(
        (0..3)
            .map(|y| {
                (
                    SubChunkKey::from_chunk(column, y),
                    mesh.clone(),
                    meshing::PackedBiomeRecord::fallback(),
                )
            })
            .collect(),
    );
    stream.load_publication_fixture_column(column);
    stream.service_publication_fixture_completions();
    assert_eq!(stream.pending_mesh_change_count(), 3);

    let budget = ChunkUploadBudget::new(8, config.maximum_frame_bytes)
        .with_zero_byte_operations_per_frame(1);
    let far_mesh = Arc::new(TileMesh {
        coverage: far_terrain::TileCoverage::Complete,
        key: TileKey::containing(
            0,
            column.x * COLUMN_SIDE as i32,
            column.z * COLUMN_SIDE as i32,
        ),
        revision: 1,
        quads: vec![[1 << 14 | 1 << 21, 64]].into_boxed_slice(),
        colours: vec![100; TILE_CELLS * TILE_CELLS * 4].into_boxed_slice(),
        min_y: 0,
        max_y: 64,
    });
    let mut app = publication_app(
        ChunkTextureAssets::new(assets),
        ChunkBiomeTints::from_resolved_with_identity(
            &stream.resolved_biome_tints_snapshot(),
            stream.biome_tint_identity(),
        ),
        budget,
        FarTerrainScene {
            tiles: vec![far_mesh.clone()].into(),
            near: Arc::new(NearMask::around([0, 0], 32, 6, [])),
            ..Default::default()
        },
    );
    publish_changes(&mut app, &mut stream);
    settle_publication_noop_frame(&mut app);
    assert_eq!(acknowledge(&app, &mut stream), 3);
    assert_near_geometry(&mut app, 3);
    assert!(stream.near_drawn_columns().1.any(|drawn| drawn == column));
    let pos = ColumnPos::new(column.x, column.z);
    assert!(
        app.world()
            .resource::<FarTerrainScene>()
            .residency
            .ready(pos, &far_mesh)
    );

    stream.set_full_detail_radius(Some(1));
    test_time::eventually("the current outgoing summary", || {
        stream.poll([8.0, 64.0, 8.0], 0);
        stream.drain_far_summaries(&mut Vec::new());
        stream.far_handoffs().any(|ready| ready == column)
    });
    let mut far = FarTerrain::new(None);
    refresh_mask(&mut app, &mut far, &stream);
    assert!(far.near.contains(pos));
    settle_publication_noop_frame(&mut app);
    assert_column_coverage(&mut app, column, true);
    stream.finish_far_handoff(column);
    refresh_mask(&mut app, &mut far, &stream);
    assert!(!stream.near_drawn_columns().1.any(|drawn| drawn == column));
    assert!(
        !far.near.contains(pos),
        "the CPU mask no longer owns draw publication"
    );

    for frame in 2..5 {
        allowance.begin_frame(frame, 0, 0, 0, config.maximum_frame_items);
        stream.poll([8.0, 64.0, 8.0], 3);
        assert_eq!(stream.pending_mesh_change_count(), 0);
        refresh_mask(&mut app, &mut far, &stream);
        settle_publication_noop_frame(&mut app);
        assert_near_geometry(&mut app, 3);
        assert_column_coverage(&mut app, column, false);
        assert_eq!(acknowledge(&app, &mut stream), 0);
    }
    allowance.begin_frame(5, 3, 0, 3, config.maximum_frame_items);
    test_time::eventually("the admitted removal batch", || {
        stream.poll([8.0, 64.0, 8.0], 3);
        stream.pending_mesh_change_count() == 3
    });
    publish_changes(&mut app, &mut stream);
    app.world_mut()
        .insert_resource(budget.with_zero_byte_operations_per_frame(0));
    for _ in 0..3 {
        refresh_mask(&mut app, &mut far, &stream);
        settle_publication_noop_frame(&mut app);
        assert_near_geometry(&mut app, 3);
        assert_column_coverage(&mut app, column, false);
        assert_eq!(acknowledge(&app, &mut stream), 0);
        assert_eq!(app.world().resource::<ChunkRenderQueue>().retained_len(), 3);
    }
    app.world_mut().insert_resource(budget);
    for remaining in (0..3).rev() {
        settle_publication_noop_frame(&mut app);
        assert_near_geometry(&mut app, remaining);
        assert_column_coverage(&mut app, column, false);
        assert_eq!(acknowledge(&app, &mut stream), 1);
        refresh_mask(&mut app, &mut far, &stream);
        assert_column_coverage(&mut app, column, false);
    }
    settle_publication_noop_frame(&mut app);
    assert!(
        !app.sub_app(RenderApp)
            .world()
            .resource::<FarTerrainScene>()
            .near
            .contains(pos)
    );
    assert_eq!(allowance.live_permits(), 0);
}

/// A new section in an existing column must reach both receipt retention and near admission.
#[test]
fn publication_membership_growth_and_retirement_refresh_cached_sources_and_targets() {
    let mut stream = WorldStream::new(protocol::WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    let first = SubChunkKey::new(0, 0, 0, 0);
    let second = SubChunkKey { y: 1, ..first };
    let a = stream.stage_publication_fixture_known_air(first);
    stream.load_publication_fixture_column(first.chunk());
    let mut far = FarTerrain::new(None);
    let mut scene = FarTerrainScene::default();
    far.publish_target(&stream, 32, &mut scene);
    let source = scene.publications.clone().unwrap();
    source.record(first, a.generation, true);
    let before = scene.target.clone().unwrap();
    far.publish_target(&stream, 32, &mut scene);
    assert!(Arc::ptr_eq(&before, scene.target.as_ref().unwrap()));

    let b = stream.stage_publication_fixture_known_air(second);
    far.publish_target(&stream, 32, &mut scene);
    let grown = scene.target.clone().unwrap();
    assert!(!Arc::ptr_eq(&before, &grown));
    assert!(source.same_source(scene.publications.as_ref().unwrap()));
    assert_eq!(grown.near[&first.chunk()].len(), 2);
    assert_eq!(grown.near[&first.chunk()][&second], b.generation);
    source.record(second, b.generation, true);
    let (revision, empty) = source.changed_empty(None).unwrap();
    assert_eq!(empty.len(), 2, "the newly retained empty receipt survives");
    let mut publication = far_terrain::TerrainPublication::default();
    publication.replace_empty(empty);
    publication.publish(&grown, &Default::default(), |_| true);
    assert!(publication.draws_near(first.chunk()));

    let stamp = stream.terrain_publication_source();
    stream.stage_publication_fixture_known_air(second);
    assert_eq!(
        stream.terrain_publication_source(),
        stamp,
        "a resend keeps membership"
    );
    far.publish_source(Some(&stream), &mut scene);
    assert!(source.changed_empty(Some(revision)).is_none());

    stream.evict_publication_fixture_column(first.chunk());
    far.publish_target(&stream, 32, &mut scene);
    assert!(scene.target.as_ref().unwrap().retained.is_empty());
    assert!(scene.target.as_ref().unwrap().near.is_empty());
    assert!(source.changed_empty(None).unwrap().1.is_empty());
    source.record(second, b.generation + 1, true);
    assert!(
        source.changed_empty(None).unwrap().1.is_empty(),
        "late retired receipt stays pruned"
    );
}

/// Equal counters in a replacement stream cannot reuse another stream's target or receipts.
#[test]
fn a_new_stream_republishes_targets_even_when_section_counters_match() {
    let bootstrap = protocol::WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    };
    let key = SubChunkKey::new(0, 0, 0, 0);
    let mut first = WorldStream::new(bootstrap);
    let mut second = WorldStream::new(bootstrap);
    for stream in [&mut first, &mut second] {
        stream.stage_publication_fixture_known_air(key);
        stream.load_publication_fixture_column(key.chunk());
    }
    assert_eq!(
        first.terrain_publication_revision(),
        second.terrain_publication_revision()
    );
    let mut far = FarTerrain::new(None);
    let mut scene = FarTerrainScene::default();
    far.publish_target(&first, 32, &mut scene);
    let target = scene.target.clone().unwrap();
    let source = scene.publications.clone().unwrap();
    source.record(key, 1, true);
    far.publish_target(&second, 32, &mut scene);
    assert!(!Arc::ptr_eq(&target, scene.target.as_ref().unwrap()));
    assert!(!source.same_source(scene.publications.as_ref().unwrap()));
    assert!(
        scene
            .publications
            .as_ref()
            .unwrap()
            .changed_empty(None)
            .unwrap()
            .1
            .is_empty()
    );
}

/// First-delivered air reaches the real render mask without a mesh or a rebuilt far tile.
#[test]
fn first_delivered_air_masks_cached_terrain_at_either_mesh_radius() {
    let sections = protocol::vanilla_dimension_range(0)
        .unwrap()
        .sub_chunk_count;
    for x in [0, 10] {
        for mode in [
            protocol::LevelChunkMode::Inline { count: 0 },
            protocol::LevelChunkMode::Inline { count: 1 },
            protocol::LevelChunkMode::LimitedRequests { highest: 0 },
        ] {
            let config = PublicationServiceConfig::DEFAULT;
            let allowance = PublicationAllowance::new(config);
            allowance.begin_frame(1, sections, 0, sections, config.maximum_frame_items);
            let assets = Arc::new(assets::RuntimeAssets::diagnostic());
            let mut stream = WorldStream::new_with_assets(
                protocol::WorldBootstrap {
                    local_player_unique_id: 1,
                    dimension: 0,
                    local_player_runtime_id: 1,
                    player_position: [0.0; 3],
                    world_spawn_position: [0; 3],
                    air_network_id: 0,
                    block_network_ids_are_hashes: false,
                },
                Arc::clone(&assets),
                [0.0; 3],
                None,
            );
            stream.set_far_terrain_enabled(true);
            stream.set_full_detail_radius(Some(1));
            stream.set_publication_allowance(allowance.clone());
            let column = ChunkKey::new(0, x, 0);
            let mut payload = if matches!(mode, protocol::LevelChunkMode::Inline { count: 1 }) {
                let base = protocol::vanilla_dimension_range(0)
                    .unwrap()
                    .base_sub_chunk_y;
                vec![9, 0, i8::try_from(base).unwrap() as u8]
            } else {
                Vec::new()
            };
            payload.extend([1, 0]);
            payload.extend(std::iter::repeat_n(0xff, sections - 1));
            payload.push(0);
            stream
                .submit(
                    1,
                    protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                        dimension: 0,
                        x,
                        z: 0,
                        mode,
                        payload,
                    }),
                )
                .unwrap();
            test_time::eventually("first air delivery committed", || {
                stream.poll([0.0; 3], 0);
                stream.terrain_publication_sections().count() >= sections
            });
            let position = ColumnPos::new(x, 0);
            let tile = Arc::new(TileMesh {
                coverage: far_terrain::TileCoverage::Partial(vec![position].into_boxed_slice()),
                key: TileKey::containing(0, x * COLUMN_SIDE as i32, 0),
                revision: 1,
                quads: vec![[1 << 14 | 1 << 21, 64]].into_boxed_slice(),
                colours: vec![100; TILE_CELLS * TILE_CELLS * 4].into_boxed_slice(),
                min_y: 0,
                max_y: 64,
            });
            let mut scene = FarTerrainScene {
                tiles: vec![tile].into(),
                ..Default::default()
            };
            let mut far = FarTerrain::new(None);
            far.publish_target(&stream, 32, &mut scene);
            let mut app = publication_app(
                ChunkTextureAssets::new(assets),
                ChunkBiomeTints::default(),
                ChunkUploadBudget::new(sections, 0).with_zero_byte_operations_per_frame(sections),
                scene,
            );
            settle_publication_noop_frame(&mut app);
            assert_eq!(
                publication_terrain_draws(&mut app, column),
                (vec![], true),
                "cached terrain remains until real empty publications arrive"
            );
            test_time::eventually("permitted empty publications", || {
                stream.poll([0.0; 3], sections);
                stream.pending_mesh_change_count() == sections
            });
            publish_changes(&mut app, &mut stream);
            settle_publication_noop_frame(&mut app);
            assert_eq!(
                publication_terrain_draws(&mut app, column),
                (vec![], false),
                "first-delivered air masks cached terrain at {x}, {mode:?}"
            );
            assert_near_geometry(&mut app, 0);
            let actual = publication_render_terminal_snapshot(&mut app);
            assert_eq!((actual.near_updates, actual.near_upload_bytes), (0, 0));
            assert_eq!(acknowledge(&app, &mut stream), sections);
            refresh_mask(&mut app, &mut far, &stream);
            settle_publication_noop_frame(&mut app);
            assert_eq!(publication_terrain_draws(&mut app, column), (vec![], false));
            assert_eq!(allowance.live_permits(), 0);
        }
    }
}

/// Far tile revisions must do constant allocation work regardless of loaded section count.
#[test]
fn far_tile_changes_reuse_section_requirements() {
    let mut work = Vec::new();
    for count in [1, 128] {
        let mut stream = WorldStream::new(protocol::WorldBootstrap {
            local_player_unique_id: 1,
            dimension: 0,
            local_player_runtime_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        for x in 0..count {
            let key = SubChunkKey::new(0, x, 0, 0);
            stream.stage_publication_fixture_known_air(key);
            stream.load_publication_fixture_column(key.chunk());
        }
        let mut far = FarTerrain::new(None);
        let mut scene = FarTerrainScene::default();
        far.publish_target(&stream, 256, &mut scene);
        let before = scene.target.clone().unwrap();
        far.tiles.clear();
        let (_, allocations) = test_alloc::measure(|| far.publish_target(&stream, 256, &mut scene));
        let after = scene.target.as_ref().unwrap();
        assert!(!Arc::ptr_eq(&before, after));
        assert_eq!(after.near.len(), count as usize);
        assert!(Arc::ptr_eq(&before.near, &after.near));
        assert!(Arc::ptr_eq(&before.retained, &after.retained));
        work.push(allocations);
    }
    assert_eq!(
        work[0], work[1],
        "far-only updates must not rescan section requirements"
    );
}
