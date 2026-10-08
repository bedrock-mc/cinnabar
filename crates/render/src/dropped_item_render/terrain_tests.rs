use super::*;
use crate::{
    ChunkRenderPlugin, ChunkRenderQueue, ChunkUploadAcknowledgements, ChunkUploadPriority,
    ChunkUploadToken, TerrainItemInstance, TerrainItemTransition, dropped_item_transform,
    publication_noop_render_plugin, publication_render_terminal_snapshot,
    settle_publication_noop_frame,
};
use bevy::{
    asset::{AssetPlugin, Assets},
    camera::{CameraPlugin, RenderTarget},
    core_pipeline::CorePipelinePlugin,
    image::ImagePlugin,
    mesh::MeshPlugin,
    window::WindowPlugin,
};
use meshing::{ChunkBiomeTintIdentity, ChunkMesh, PackedBiomeRecord};
use render_api::{PublicationAllowance, PublicationServiceConfig};
use render_model::DroppedItemSprite;
use std::{sync::Arc, time::Instant};
use world::SubChunkKey;

#[derive(Resource, Default)]
struct QueuedItemWitness(usize);

fn capture_queued_items(
    phases: Res<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    mut witness: ResMut<QueuedItemWitness>,
) {
    let draw = draw_functions.read().id::<DrawItemCommands>();
    witness.0 = phases
        .0
        .values()
        .flat_map(|phase| phase.non_mesh_items.iter())
        .filter(|((key, _), _)| key.draw_function == draw)
        .map(|(_, entities)| entities.entities.len())
        .sum();
}

fn fixture() -> App {
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
        .add_plugins((ChunkRenderPlugin::default(), DroppedItemRenderPlugin));
    app.sub_app_mut(RenderApp)
        .init_resource::<QueuedItemWitness>()
        .add_systems(
            Render,
            capture_queued_items
                .in_set(RenderSystems::Queue)
                .after(queue_items),
        );
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            32,
            32,
            TextureFormat::Rgba8Unorm,
            None,
        ));
    app.world_mut().spawn((
        Camera3d::default(),
        Camera::default(),
        RenderTarget::Image(image.into()),
        Msaa::Off,
        Transform::from_xyz(3.0, 3.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    app.world_mut().resource_mut::<DroppedItemScene>().publish(
        1,
        Arc::from([DroppedItemModel::Sprite(DroppedItemSprite {
            width: 1,
            height: 1,
            rgba8: Arc::from([255u8; 4]),
        })]),
        &[],
        &[],
        1.0,
    );
    app.finish();
    app.cleanup();
    for _ in 0..2 {
        settle_publication_noop_frame(&mut app);
    }
    app
}

fn instance() -> crate::DroppedItemInstance {
    crate::DroppedItemInstance {
        model: 0,
        world_from_item: dropped_item_transform([0.0; 3], 0.0, 1.0),
        block_level: 15,
        sky_level: 15,
        overlay_rgba8: 0,
    }
}

fn candidate(app: &mut App, visible: bool, transitions: &[TerrainItemTransition]) {
    app.world_mut()
        .resource_mut::<DroppedItemScene>()
        .publish_terrain_instances(
            1,
            &[TerrainItemInstance {
                instance: instance(),
                visible,
                transitions: Arc::from(transitions),
            }],
        );
}

fn mesh() -> ChunkMesh {
    let source = world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 });
    meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &assets::RuntimeAssets::diagnostic(),
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &source,
    )
}

fn upload(app: &mut App, key: SubChunkKey, generation: u64, mesh: ChunkMesh) {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let allowance = PublicationAllowance::new(config);
    allowance.begin_frame(
        generation,
        1,
        config.maximum_frame_bytes,
        1,
        config.maximum_frame_items,
    );
    let biome = PackedBiomeRecord::fallback();
    let bytes = ChunkRenderQueue::upload_byte_len(&mesh, &biome);
    let permit = if mesh.is_empty() {
        allowance.try_admit_zero_byte().unwrap()
    } else {
        allowance.try_admit_payload(bytes).unwrap()
    };
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked_with_biome_identity_permitted(
            key,
            mesh,
            biome,
            ChunkBiomeTintIdentity::default(),
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation,
                dirty_since: Instant::now(),
            },
            permit,
        )
        .unwrap();
}

fn draw_count(app: &App) -> usize {
    app.sub_app(RenderApp)
        .world()
        .resource::<ItemGpu>()
        .draws
        .len()
}

fn queued_items(app: &App) -> usize {
    app.sub_app(RenderApp)
        .world()
        .resource::<QueuedItemWitness>()
        .0
}

#[test]
fn current_frame_item_appearance_queues_without_prior_gpu_draws() {
    let mut app = fixture();
    assert_eq!(draw_count(&app), 0);
    app.world_mut().resource_mut::<DroppedItemScene>().publish(
        1,
        Arc::from([]),
        &[instance()],
        &[],
        1.0,
    );
    settle_publication_noop_frame(&mut app);
    assert_eq!(draw_count(&app), 1);
    assert_eq!(
        queued_items(&app),
        1,
        "this frame's first item must reach its opaque draw phase"
    );
}

#[test]
fn terrain_create_draws_in_the_frame_its_source_mesh_is_removed() {
    let mut app = fixture();
    let key = SubChunkKey::new(0, 0, 0, 0);
    candidate(
        &mut app,
        false,
        &[TerrainItemTransition {
            key,
            generation: 2,
            visible: true,
        }],
    );
    upload(&mut app, key, 1, mesh());
    settle_publication_noop_frame(&mut app);
    assert_eq!(draw_count(&app), 0);
    upload(&mut app, key, 2, ChunkMesh::default());
    settle_publication_noop_frame(&mut app);
    assert!(
        publication_render_terminal_snapshot(&mut app)
            .allocation_manifest
            .is_empty()
    );
    assert_eq!(
        draw_count(&app),
        1,
        "source removal and actor CREATE must share a render frame"
    );
    assert_eq!(queued_items(&app), 1);
    let _ = app
        .world()
        .resource::<ChunkUploadAcknowledgements>()
        .drain();
    settle_publication_noop_frame(&mut app);
    assert_eq!(
        draw_count(&app),
        1,
        "main-world ack draining cannot erase render visibility"
    );
}

#[test]
fn terrain_destroy_hides_in_the_frame_its_landing_mesh_is_uploaded() {
    let mut app = fixture();
    let key = SubChunkKey::new(0, 0, 0, 0);
    candidate(
        &mut app,
        true,
        &[TerrainItemTransition {
            key,
            generation: 2,
            visible: false,
        }],
    );
    settle_publication_noop_frame(&mut app);
    assert_eq!(draw_count(&app), 1);
    upload(&mut app, key, 2, mesh());
    settle_publication_noop_frame(&mut app);
    assert_eq!(
        publication_render_terminal_snapshot(&mut app).allocation_manifest,
        vec![(key, 2)]
    );
    assert_eq!(
        draw_count(&app),
        0,
        "published terrain must replace the actor in the same render frame"
    );
}

#[test]
fn terrain_handoff_uses_newer_generation_and_global_transition_order() {
    let mut app = fixture();
    let source = SubChunkKey::new(0, 9, 0, 0);
    let destination = SubChunkKey::new(0, -9, 0, 0);
    candidate(
        &mut app,
        false,
        &[TerrainItemTransition {
            key: source,
            generation: 3,
            visible: true,
        }],
    );
    upload(&mut app, source, 2, mesh());
    settle_publication_noop_frame(&mut app);
    assert_eq!(draw_count(&app), 0, "stale geometry cannot release CREATE");
    upload(&mut app, source, 4, ChunkMesh::default());
    settle_publication_noop_frame(&mut app);
    assert_eq!(
        draw_count(&app),
        1,
        "coalesced newer geometry releases earlier CREATE"
    );
    candidate(
        &mut app,
        true,
        &[
            TerrainItemTransition {
                key: source,
                generation: 3,
                visible: true,
            },
            TerrainItemTransition {
                key: destination,
                generation: 2,
                visible: false,
            },
        ],
    );
    upload(&mut app, destination, 2, mesh());
    settle_publication_noop_frame(&mut app);
    assert_eq!(
        draw_count(&app),
        0,
        "wire order outranks subchunk-key order"
    );
}

#[test]
fn terrain_handoff_prunes_unreferenced_keys_and_resets_sessions() {
    let mut app = fixture();
    let key = SubChunkKey::new(0, 0, 0, 0);
    let transition = TerrainItemTransition {
        key,
        generation: 1,
        visible: true,
    };
    candidate(&mut app, false, &[transition]);
    upload(&mut app, key, 1, ChunkMesh::default());
    settle_publication_noop_frame(&mut app);
    assert_eq!(draw_count(&app), 1);
    app.world_mut()
        .resource_mut::<DroppedItemScene>()
        .publish_terrain_instances(1, &[]);
    settle_publication_noop_frame(&mut app);
    candidate(&mut app, false, &[transition]);
    settle_publication_noop_frame(&mut app);
    assert_eq!(
        draw_count(&app),
        0,
        "unreferenced mesh stamps cannot resurrect a later candidate"
    );
    upload(&mut app, key, 2, ChunkMesh::default());
    settle_publication_noop_frame(&mut app);
    assert_eq!(draw_count(&app), 1);
    app.world_mut()
        .resource_mut::<DroppedItemScene>()
        .publish_terrain_instances(
            2,
            &[TerrainItemInstance {
                instance: instance(),
                visible: false,
                transitions: Arc::from([transition]),
            }],
        );
    settle_publication_noop_frame(&mut app);
    assert_eq!(
        draw_count(&app),
        0,
        "prior-session uploads cannot satisfy current-session transitions"
    );
}

#[test]
fn terrain_and_ordinary_items_share_the_gpu_instance_capacity() {
    let mut app = fixture();
    app.world_mut().resource_mut::<DroppedItemScene>().publish(
        1,
        Arc::from([]),
        &vec![instance(); MAX_DROPPED_ITEM_INSTANCES],
        &[],
        1.0,
    );
    app.world_mut()
        .resource_mut::<DroppedItemScene>()
        .publish_terrain_instances(
            1,
            &vec![
                TerrainItemInstance {
                    instance: instance(),
                    visible: true,
                    transitions: Arc::from([])
                };
                MAX_DROPPED_ITEM_INSTANCES
            ],
        );
    settle_publication_noop_frame(&mut app);
    assert_eq!(draw_count(&app), MAX_DROPPED_ITEM_INSTANCES);
    app.world_mut().resource_mut::<DroppedItemScene>().clear();
    settle_publication_noop_frame(&mut app);
    assert_eq!(draw_count(&app), 0);
    assert_eq!(queued_items(&app), 0);
}

#[test]
fn immediate_empty_mesh_handoff_survives_ack_mailbox_draining() {
    let mut app = fixture();
    let key = SubChunkKey::new(0, 0, 0, 0);
    candidate(
        &mut app,
        false,
        &[TerrainItemTransition {
            key,
            generation: 1,
            visible: true,
        }],
    );
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_update_tracked_with_biome_identity(
            key,
            ChunkMesh::default(),
            PackedBiomeRecord::fallback(),
            ChunkBiomeTintIdentity::default(),
            ChunkUploadPriority::new(0.0),
            ChunkUploadToken {
                generation: 1,
                dirty_since: Instant::now(),
            },
        )
        .unwrap();
    settle_publication_noop_frame(&mut app);
    assert_eq!(draw_count(&app), 1);
    let _ = app
        .world()
        .resource::<ChunkUploadAcknowledgements>()
        .drain();
    settle_publication_noop_frame(&mut app);
    assert_eq!(draw_count(&app), 1);
}
