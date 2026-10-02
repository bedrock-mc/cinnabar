//! Exercises the production graph on an offscreen native GPU, without a server.
use super::*;
use crate::actor::ActorRigVertexSegments;
use crate::{
    ActorDrawManifestEntry, ActorGpuInstance, ActorPresentationGate, ActorRenderFrame,
    ActorRenderIdentity, ActorRenderPlugin, ActorRigGeometrySpan, ActorRigRenderFrame,
    ActorRigRoute, ActorRigVertex, AtmospherePlugin, ChunkRenderPlugin, ChunkRenderQueue,
    ChunkTextureAssets, ChunkUploadPriority, EntityRigId, HandRigLight, HandRigRenderPlugin,
    HandRigScene, NametagRecord, NametagScene, ParticleGpuFrame, ParticleRenderPlugin,
    ParticleSystem, STANDARD_SKIN_BYTES, UI_BLEND_ALPHA, UI_BLEND_INVERT, UiRenderBatch,
    UiRenderInput, UiRenderPlugin, UiRenderScene, UiRenderStats, UiRenderVertex, UiScissor,
    UiTextureCatalog, UiTexturePage, ViewmodelCompletionGate, ViewmodelGeometry,
    ViewmodelRenderPlugin, ViewmodelScene, ViewmodelToken, update_particle_frame,
};
use bevy::{
    asset::AssetPlugin,
    camera::{Camera3dDepthTextureUsage, CameraPlugin, RenderTarget},
    core_pipeline::{CorePipelinePlugin, tonemapping::Tonemapping},
    mesh::MeshPlugin,
    post_process::{PostProcessPlugin, bloom::Bloom},
    render::{
        RenderPlugin,
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::*,
        renderer::RenderDevice,
        view::Hdr,
    },
    window::WindowPlugin,
};
use std::sync::Arc;

const VIEWPORT: [u32; 2] = [128, 96];

#[derive(Resource, Default)]
struct Captured(Vec<u8>);

/// Native rendering uses the same plugins and attachments as the client.
fn app() -> Option<(App, Entity)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    if bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .is_err()
    {
        assert!(
            std::env::var_os("CINNABAR_REQUIRE_ENHANCED_GPU").is_none(),
            "native Enhanced GPU validation was required"
        );
        eprintln!("Enhanced populated smoke skipped: no native adapter");
        return None;
    }
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins(AssetPlugin::default())
        .add_plugins(RenderPlugin {
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
            PostProcessPlugin,
        ))
        .add_plugins((
            ChunkRenderPlugin::new(1),
            AtmospherePlugin,
            ActorRenderPlugin,
            UiRenderPlugin,
            ViewmodelRenderPlugin,
            HandRigRenderPlugin,
            ParticleRenderPlugin,
            EnhancedRenderPlugin,
        ));
    let image = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(Image::new_target_texture(
            VIEWPORT[0],
            VIEWPORT[1],
            TextureFormat::Rgba8Unorm,
            Some(TextureFormat::Rgba8UnormSrgb),
        ));
    let camera = app
        .world_mut()
        .spawn((
            Camera3d {
                depth_texture_usages: Camera3dDepthTextureUsage::from(
                    TextureUsages::RENDER_ATTACHMENT
                        | TextureUsages::TEXTURE_BINDING
                        | TextureUsages::COPY_SRC,
                ),
                ..default()
            },
            Camera::default(),
            RenderTarget::Image(image.clone().into()),
            Msaa::Off,
            Hdr,
            Tonemapping::None,
            EnhancedRendering::default(),
            Bloom::default(),
            Transform::from_xyz(0.0, 1.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
        ))
        .id();
    app.init_resource::<Captured>();
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&image)
        .unwrap()
        .texture_descriptor
        .usage |= TextureUsages::COPY_SRC;
    app.world_mut().spawn(Readback::texture(image)).observe(
        |event: On<ReadbackComplete>, mut capture: ResMut<Captured>| {
            capture.0.clone_from(&event.data);
        },
    );
    app.finish();
    app.cleanup();
    Some((app, camera))
}

/// One skinned triangle activates actor uploads and the HDR opaque draw.
fn rig() -> ActorRigRenderFrame {
    let identity = ActorRenderIdentity {
        session_id: 1,
        dimension: 0,
        runtime_id: 1,
        spawn_revision: 1,
        ingress_sequence: 1,
        source_tick: None,
        movement_revision: 0,
        pose_generation: 1,
        layer: 0,
    };
    ActorRigRenderFrame {
        frame_generation: 1,
        geometry_revision: 1,
        instances: Arc::from([ActorGpuInstance {
            world_from_actor: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            ..default()
        }]),
        previous_bones: Arc::from([[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]]),
        current_bones: Arc::from([[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]]),
        geometry_vertices: ActorRigVertexSegments::from_vertices([
            ActorRigVertex {
                position: [-0.5, 0.0, 0.0],
                ..default()
            },
            ActorRigVertex {
                position: [0.5, 0.0, 0.0],
                ..default()
            },
            ActorRigVertex {
                position: [0.0, 1.0, 0.0],
                ..default()
            },
        ]),
        geometry_spans: Arc::from([ActorRigGeometrySpan {
            first_vertex: 0,
            vertex_count: 3,
        }]),
        manifest: Arc::from([ActorDrawManifestEntry {
            identity,
            rig: EntityRigId(0),
            completed_tick: 1,
            reset_generation: 1,
            route: ActorRigRoute::Compiled,
            instance_index: 0,
            bone_count: 1,
            previous_bone_base: 0,
            current_bone_base: 0,
        }]),
        maximum_vertex_count: 3,
        ..default()
    }
}

/// Exercises the held-cube path when the installed world carrier is available.
fn held_cube(app: &mut App, camera: Entity) -> Option<ViewmodelToken> {
    let manifest: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../assets/bedrock-target.json")).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(manifest["artifacts"]["world_assets"].as_str().unwrap());
    let Ok(bytes) = std::fs::read(path) else {
        return None;
    };
    let assets = Arc::new(assets::RuntimeAssets::decode(&bytes).unwrap());
    let (geometry, skin) = (0..assets.visual_count())
        .find_map(|id| ViewmodelGeometry::opaque_cube(&assets, assets::BlockVisualId(id as u32)))
        .expect("the installed carrier has ordinary opaque cubes");
    app.world_mut()
        .insert_resource(ChunkTextureAssets::new(assets));
    let token = ViewmodelToken {
        session: 1,
        actor_session: 1,
        dimension: 0,
        runtime: 1,
        spawn: 1,
        owner: camera,
        viewport: VIEWPORT,
        samples: 1,
        hdr: true,
        skin: skin.identity(),
        geometry: ViewmodelScene::geometry_identity(&geometry),
        revision: 1,
    };
    let gate = app.world().resource::<ViewmodelCompletionGate>().clone();
    assert!(
        app.world_mut()
            .resource_mut::<ViewmodelScene>()
            .publish(token, &skin, &geometry, &gate)
    );
    Some(token)
}

/// Publishes world text and a HUD rectangle through the production UI composite.
fn ui(app: &mut App) {
    let vertices =
        [[8.0, 8.0], [32.0, 8.0], [32.0, 24.0], [8.0, 24.0]].map(|position| UiRenderVertex {
            position,
            uv: [0.0, 0.0],
            color: [255; 4],
            style_flags: 0,
            clip_z: 0.5,
            clip_w: 1.0,
            alpha_cutoff: 0.0,
            model_light: 1.0,
        });
    let mut vertices = vertices.to_vec();
    let template = vertices[0];
    vertices.extend(
        [[100.0, 70.0], [112.0, 70.0], [112.0, 90.0], [100.0, 90.0]]
            .into_iter()
            .zip([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]])
            .map(|(position, uv)| UiRenderVertex {
                position,
                uv,
                ..template
            })
            .collect::<Vec<_>>(),
    );
    let mut projected = UiRenderBatch::new(
        0,
        UiScissor::new(0, 0, VIEWPORT[0], VIEWPORT[1]),
        0,
        6,
        UI_BLEND_ALPHA,
    )
    .with_depth_test(true);
    projected.depth_write = 1;
    let input = UiRenderInput {
        revision: 1,
        viewport_size: VIEWPORT,
        safe_area: [0; 4],
        vertices: Arc::from(vertices),
        indices: Arc::from([
            0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7, 0, 1, 2, 0, 2, 3, 0, 1, 2, 0, 2, 3,
        ]),
        batches: Arc::from([
            projected,
            UiRenderBatch::new(
                0,
                UiScissor::new(0, 0, VIEWPORT[0], VIEWPORT[1]),
                6,
                6,
                UI_BLEND_ALPHA,
            ),
            UiRenderBatch::new(
                0,
                UiScissor::new(0, 0, VIEWPORT[0], VIEWPORT[1]),
                12,
                6,
                UI_BLEND_ALPHA,
            )
            .with_depth_test(true)
            .with_depth_write(true)
            .with_isolated_depth_scope(Some(1)),
            UiRenderBatch::new(
                0,
                UiScissor::new(0, 0, VIEWPORT[0], VIEWPORT[1]),
                18,
                6,
                UI_BLEND_INVERT,
            ),
        ]),
        textures: Arc::new(
            UiTextureCatalog::new(
                vec![UiTexturePage::owned([1, 1], Arc::from([255; 4])).unwrap()],
                1,
            )
            .unwrap(),
        ),
    };
    let gate = app.world().resource::<ViewmodelCompletionGate>().clone();
    {
        let mut scene = app.world_mut().resource_mut::<ViewmodelScene>();
        if scene.is_opaque_cube() {
            assert!(scene.bind_cube_cpu_fallback(&input, 0, [0, 0, 1, 1], &gate));
        }
    }
    let stats = app.world().resource::<UiRenderStats>().clone();
    app.world_mut()
        .resource_mut::<UiRenderScene>()
        .publish(input, &stats)
        .unwrap();
}

#[test]
fn enhanced_from_startup_renders_populated_world_on_native_gpu() {
    let Some((mut app, camera)) = app() else {
        return;
    };
    // Settle the startup menu graph before the joined world's content arrives.
    for _ in 0..3 {
        app.update();
    }
    let liquid = meshing::PackedLiquidQuad::try_pack(
        [0, 0, 0],
        meshing::Face::PositiveY,
        [255; 4],
        assets::DIAGNOSTIC_MATERIAL,
        0,
        [0, 0],
        false,
    )
    .unwrap();
    let diagnostic = assets::RuntimeAssets::diagnostic();
    let cube = meshing::mesh_sub_chunk(
        &meshing::BlockClassifier::new(0),
        &diagnostic,
        assets::NetworkIdMode::Sequential,
        &meshing::Neighbourhood::empty(),
        &world::SubChunk::decode(&[9, 1, 0, 1, 2], &world::RawBlockIds { air: 0 }),
    );
    assert!(!cube.cube_quads().is_empty());
    let mesh = meshing::ChunkMesh::from_streams(
        cube.cube_quads().to_vec(),
        vec![],
        vec![],
        vec![],
        vec![liquid],
        vec![meshing::PackedQuadLighting::new([0; 4])],
        Default::default(),
    );
    app.world_mut()
        .resource_mut::<ChunkRenderQueue>()
        .try_insert(
            world::SubChunkKey::new(0, 0, 0, 0),
            mesh,
            ChunkUploadPriority::new(0.0),
        )
        .unwrap();
    let rig = rig();
    let skin: Arc<[u8]> = vec![255; STANDARD_SKIN_BYTES].into();
    app.world_mut().insert_resource(ActorRenderFrame {
        rig: rig.clone(),
        skins_rgba8: skin.clone(),
        skin_revision: 1,
        instance_pages: Arc::from([0]),
        ..default()
    });
    assert!(app.world_mut().resource_mut::<HandRigScene>().publish(
        rig,
        skin,
        HandRigLight {
            block_level: 15,
            sky_level: 15,
            daylight: 1.0,
            pad: 0
        },
        1.2,
        1
    ));
    app.world_mut().insert_resource(NametagScene {
        records: (0..4)
            .map(|index| NametagRecord {
                anchor: [0.0, 1.5, 0.0],
                rect: [-8.0, -4.0, 8.0, 4.0],
                uv: [0.0, 0.0, if index % 2 == 0 { -1.0 } else { 1.0 }, 1.0],
                color: [1.0; 4],
                text: index % 2,
                ..default()
            })
            .collect(),
        see_through: 2,
        ..default()
    });
    let mut particles = ParticleSystem::default();
    assert!(particles.register_effect(br#"{"particle_effect":{"description":{"identifier":"fixture:burst","basic_render_parameters":{"material":"particles_alpha","texture":"x"}},"components":{"minecraft:emitter_lifetime_once":{"active_time":1},"minecraft:emitter_rate_instant":{"num_particles":4},"minecraft:emitter_shape_point":{},"minecraft:particle_lifetime_expression":{"max_lifetime":5},"minecraft:particle_appearance_billboard":{"size":[0.1,0.1],"facing_camera_mode":"lookat_xyz","uv":{"uv":[0,0],"uv_size":[1,1]}}}}}"#));
    particles.spawn(&crate::particles::SpawnRequest {
        effect: "fixture:burst".into(),
        ..default()
    });
    let mut frame = ParticleGpuFrame::default();
    update_particle_frame(
        &mut particles,
        &mut frame,
        0.05,
        &crate::particles::ParticleView {
            position: [0.0, 1.0, 4.0],
            right: [1.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            forward: [0.0, 0.0, -1.0],
            half_diagonal: 1.5,
        },
        &crate::particles::EmptyWorld,
    );
    assert!(frame.instance_count() > 0);
    app.world_mut().insert_resource(frame);
    let held = held_cube(&mut app, camera);
    ui(&mut app);
    for _ in 0..8 {
        app.update();
        app.sub_app(RenderApp)
            .world()
            .resource::<RenderDevice>()
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
    }
    if let Some(token) = held {
        assert!(
            app.world()
                .resource::<ViewmodelCompletionGate>()
                .completed(token),
            "the held cube must actually draw and complete"
        );
    }
    let pixels = &app.world().resource::<Captured>().0;
    assert_eq!(pixels.len(), (VIEWPORT[0] * VIEWPORT[1] * 4) as usize);
    if let Ok(path) = std::env::var("CINNABAR_ENHANCED_FRAME") {
        image::RgbaImage::from_raw(VIEWPORT[0], VIEWPORT[1], pixels.clone())
            .unwrap()
            .save(path)
            .unwrap();
    }
    assert!(
        !app.world()
            .resource::<ActorPresentationGate>()
            .drain()
            .is_empty(),
        "the opaque actor pass must actually draw and complete"
    );
    app.world_mut()
        .entity_mut(camera)
        .remove::<(EnhancedRendering, Hdr, Bloom)>();
    for _ in 0..3 {
        app.update();
    }
    app.world_mut().entity_mut(camera).insert((
        EnhancedRendering::default(),
        Hdr,
        Bloom::default(),
    ));
    for _ in 0..3 {
        app.update();
    }
    let world = app.sub_app_mut(RenderApp).world_mut();
    let cache = world.resource::<PipelineCache>();
    assert_eq!(
        cache.waiting_pipelines().count(),
        0,
        "all populated pipelines compiled"
    );
    for pipeline in cache.pipelines() {
        if let CachedPipelineState::Err(error) = &pipeline.state {
            panic!("populated pipeline failed: {error}");
        }
    }
    assert!(
        app.world()
            .resource::<UiRenderStats>()
            .snapshot()
            .draw_calls
            > 0
    );
}
