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
    publication_render_terminal_snapshot, settle_publication_noop_frame,
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
    app.world_mut().resource_mut::<FarTerrainScene>().near = Arc::clone(&far.near);
}

/// Observes actual extracted near geometry and allocations, rather than handoff membership.
fn assert_near_geometry(app: &mut App, count: usize) {
    let state = publication_render_terminal_snapshot(app);
    assert_eq!(state.extracted_manifest.len(), count);
    assert_eq!(state.allocation_manifest.len(), count);
}

#[test]
fn outgoing_mask_waits_through_delayed_dispatch_and_bounded_removal_publication() {
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
        .insert_resource(ChunkTextureAssets::new(assets))
        .insert_resource(ChunkBiomeTints::from_resolved_with_identity(
            &stream.resolved_biome_tints_snapshot(),
            stream.biome_tint_identity(),
        ))
        .insert_resource(FarTerrainScene {
            tiles: vec![far_mesh.clone()].into(),
            near: Arc::new(NearMask::around([0, 0], 32, 6, [])),
            ..Default::default()
        })
        .add_plugins(ChunkRenderPlugin::with_budget(budget))
        .add_plugins(FarTerrainRenderPlugin);
    app.finish();
    app.cleanup();
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
    stream.finish_far_handoff(column);
    refresh_mask(&mut app, &mut far, &stream);
    assert!(!stream.near_drawn_columns().1.any(|drawn| drawn == column));
    assert!(
        far.near.contains(pos),
        "scheduling a withdrawal must keep its mask"
    );

    for frame in 2..5 {
        allowance.begin_frame(frame, 0, 0, 0, config.maximum_frame_items);
        stream.poll([8.0, 64.0, 8.0], 3);
        assert_eq!(stream.pending_mesh_change_count(), 0);
        refresh_mask(&mut app, &mut far, &stream);
        settle_publication_noop_frame(&mut app);
        assert_near_geometry(&mut app, 3);
        assert!(
            far.near.contains(pos),
            "dirty removal dispatch remains delayed"
        );
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
        assert!(far.near.contains(pos), "render publication remains delayed");
        assert_eq!(acknowledge(&app, &mut stream), 0);
        assert_eq!(app.world().resource::<ChunkRenderQueue>().retained_len(), 3);
    }
    app.world_mut().insert_resource(budget);
    for remaining in (0..3).rev() {
        settle_publication_noop_frame(&mut app);
        assert_near_geometry(&mut app, remaining);
        assert!(far.near.contains(pos), "no receipt has been applied yet");
        assert_eq!(acknowledge(&app, &mut stream), 1);
        refresh_mask(&mut app, &mut far, &stream);
        assert_eq!(far.near.contains(pos), remaining != 0);
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
