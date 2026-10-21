//! Exercises sky/world composition through the real headless render app.

use bevy::render::render_phase::AddRenderCommand;
use std::collections::HashMap;

use bevy::{
    asset::{AssetId, AssetPlugin, uuid_handle},
    camera::{CameraPlugin, RenderTarget},
    core_pipeline::{
        CorePipelinePlugin,
        core_3d::{CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
    },
    ecs::{query::ROQueryItem, system::SystemParamItem},
    image::ImagePlugin,
    mesh::{Mesh, MeshPlugin},
    prelude::*,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        render_asset::RenderAssets,
        render_phase::{
            BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewBinnedRenderPhases,
        },
        render_resource::*,
        renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
        settings::RenderCreation,
        sync_world::MainEntity,
        texture::GpuImage,
        view::ExtractedView,
    },
    window::WindowPlugin,
};

use {
    crate::ChunkRenderPlugin,
    render_scene::{AtmosphereFrame, AtmosphereTextureAssets, SkyKind},
};

const SIZE: [u32; 2] = [64, 48];
const WORLD_SHADER: Handle<Shader> = uuid_handle!("c22977be-f4db-4e14-aedb-d31025ce01ca");
const WORLD_SOURCE: &str = r#"
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let xy = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return vec4(xy, WORLD_DEPTH, 1.0);
}
@fragment fn fragment() -> @location(0) vec4<f32> { return vec4(0.0, 1.0, 0.0, 1.0); }
"#;

struct Fixture {
    gpu: crate::gpu_snapshot::Gpu,
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
}

/// Retains the adapter and instance required to give Bevy the same native test device.
fn fixture() -> Option<Fixture> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = match bevy::tasks::block_on(instance.request_adapter(&default())) {
        Ok(adapter) if adapter.get_info().backend != wgpu::Backend::Noop => adapter,
        Ok(_) | Err(wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!("skipping finite skybox real render schedule: missing native GPU fixture");
            return None;
        }
        Err(error) => panic!("finite skybox render schedule adapter: {error}"),
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: wgpu::Limits {
            max_storage_buffers_per_shader_stage: crate::required_vertex_storage_buffers(),
            ..default()
        },
        ..default()
    }))
    .expect("finite skybox render schedule device");
    Some(Fixture {
        gpu: crate::gpu_snapshot::Gpu {
            device,
            queue,
            backend: adapter.get_info().backend,
        },
        instance,
        adapter,
    })
}

/// Builds the real sky schedule, optionally adding a green surface at the supplied distance.
fn app(fixture: &Fixture, msaa: Msaa, distance: Option<f32>, pack: bool) -> (App, Handle<Image>) {
    let gpu = &fixture.gpu;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins((AssetPlugin::default(), TransformPlugin))
        .add_plugins(RenderPlugin {
            render_creation: RenderCreation::manual(
                RenderDevice::from(gpu.device.clone()),
                RenderQueue::new(gpu.queue.clone()),
                RenderAdapterInfo::new(fixture.adapter.get_info()),
                RenderAdapter::new(fixture.adapter.clone()),
                RenderInstance::new(fixture.instance.clone()),
            ),
            synchronous_pipeline_compilation: true,
            ..default()
        })
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ))
        .add_plugins(ChunkRenderPlugin::new(1));
    let source = WORLD_SOURCE.replace(
        "WORLD_DEPTH",
        &(render_api::CAMERA_NEAR_PLANE_BLOCKS / distance.unwrap_or(4.0)).to_string(),
    );
    app.world_mut()
        .resource_mut::<Assets<Shader>>()
        .insert(
            WORLD_SHADER.id(),
            crate::shader_safety::from_wgsl(source, "synthetic distant world surface"),
        )
        .unwrap();
    if distance.is_some() {
        app.sub_app_mut(RenderApp)
            .add_render_command::<Opaque3d, (SetItemPipeline, DrawWorld)>()
            .add_systems(
                Render,
                queue_world
                    .before(super::queue_atmosphere)
                    .in_set(RenderSystems::Queue),
            );
    }
    let mut image =
        Image::new_target_texture(SIZE[0], SIZE[1], TextureFormat::Bgra8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
    app.world_mut().spawn((
        Camera3d::default(),
        Camera {
            clear_color: bevy::camera::ClearColorConfig::Custom(Color::srgb(1.0, 0.0, 1.0)),
            ..default()
        },
        RenderTarget::Image(image.clone().into()),
        msaa,
        Transform::default(),
        Projection::Perspective(PerspectiveProjection {
            near: render_api::CAMERA_NEAR_PLANE_BLOCKS,
            ..default()
        }),
    ));
    let skyboxes = assets::SkyboxDimension::ALL.map(|_| assets::PackSkybox {
        faces: std::array::from_fn(|_| {
            pack.then_some(assets::SkyboxFace {
                width: 1,
                height: 1,
                rgba8: Box::new([255, 0, 0, 255]),
            })
        }),
    });
    app.insert_resource(AtmosphereFrame::default().with_sky_kind(SkyKind::End));
    app.insert_resource(
        AtmosphereTextureAssets::new(super::tests::synthetic_runtime(0x20), [0x51; 32])
            .with_skyboxes(skyboxes, [0x52; 32]),
    );
    app.finish();
    app.cleanup();
    (app, image)
}

/// Queues world geometry first to reproduce the unordered sky-bin overwrite.
fn queue_world(
    cache: Res<PipelineCache>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    functions: Res<DrawFunctions<Opaque3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
    mut pipelines: Local<HashMap<(Msaa, bool), CachedRenderPipelineId>>,
) {
    let draw = functions.read().id::<(SetItemPipeline, DrawWorld)>();
    for (entity, main, view, camera, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let pipeline = *pipelines.entry((*msaa, camera.hdr)).or_insert_with(|| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("synthetic world surface".into()),
                vertex: VertexState {
                    shader: WORLD_SHADER,
                    entry_point: Some("vertex".into()),
                    ..default()
                },
                fragment: Some(FragmentState {
                    shader: WORLD_SHADER,
                    entry_point: Some("fragment".into()),
                    targets: vec![Some(ColorTargetState {
                        format: if camera.hdr {
                            render_scene::SCENE_HDR_FORMAT
                        } else {
                            render_scene::SCENE_COLOR_FORMAT
                        },
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                depth_stencil: Some(DepthStencilState {
                    format: CORE_3D_DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(CompareFunction::GreaterEqual),
                    stencil: default(),
                    bias: default(),
                }),
                multisample: MultisampleState {
                    count: msaa.samples(),
                    ..default()
                },
                ..default()
            })
        });
        phase.add(
            Opaque3dBatchSetKey {
                draw_function: draw,
                pipeline,
                material_bind_group_index: None,
                lightmap_slab: None,
                slabs: default(),
            },
            Opaque3dBinKey {
                asset_id: AssetId::<Mesh>::default().untyped(),
            },
            (entity, *main),
            InputUniformIndex::default(),
            BinnedRenderPhaseType::NonMesh,
        );
    }
}

struct DrawWorld;
impl<P: PhaseItem> RenderCommand<P> for DrawWorld {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = ();
    fn render<'w>(
        _: &P,
        _: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.draw(0..3, 0..1);
        RenderCommandResult::Success
    }
}

/// Reads the completed image after the actual production render schedule ran.
fn pixel(app: &App, image: &Handle<Image>) -> [u8; 4] {
    let world = app.sub_app(RenderApp).world();
    let device = world.resource::<RenderDevice>();
    let queue = world.resource::<RenderQueue>();
    let images = world.resource::<RenderAssets<GpuImage>>();
    let image = images.get(image).expect("prepared headless image");
    let buffer = device.create_buffer(&BufferDescriptor {
        label: Some("sky/world regression readback"),
        size: 256,
        usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &image.texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: SIZE[0] / 2,
                y: SIZE[1] / 2,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(1),
            },
        },
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    buffer.slice(..).get_mapped_range().unwrap()[..4]
        .try_into()
        .unwrap()
}

#[test]
fn end_skybox_never_overwrites_near_or_distant_opaque_geometry() {
    let Some(gpu) = fixture() else {
        return;
    };
    for msaa in [Msaa::Off, Msaa::Sample4] {
        let (mut sky, image) = app(&gpu, msaa, None, true);
        for _ in 0..8 {
            sky.update();
        }
        let colour = pixel(&sky, &image);
        assert!(
            colour[2] > 200 && colour[0] < 20 && colour[1] < 20,
            "{msaa:?} sky-only control did not draw the red pack cube: {colour:?}"
        );
        drop(sky);
        for distance in [4.0, 100.0] {
            let (mut app, image) = app(&gpu, msaa, Some(distance), true);
            for _ in 0..8 {
                app.update();
            }
            let colour = pixel(&app, &image);
            assert!(
                colour[1] > 200 && colour[0] < 20 && colour[2] < 20,
                "{msaa:?} world at {distance} blocks was covered by sky: {colour:?}"
            );
        }
    }
}

/// Covers the late stock path through the actual schedule in every dimension and MSAA mode.
#[test]
fn stock_sky_fills_clear_samples_without_covering_opaque_geometry() {
    let Some(gpu) = fixture() else {
        return;
    };
    for msaa in [Msaa::Off, Msaa::Sample4] {
        for kind in [SkyKind::Overworld, SkyKind::Nether, SkyKind::End] {
            for distance in [None, Some(4.0), Some(100.0)] {
                let (mut app, image) = app(&gpu, msaa, distance, false);
                app.insert_resource(AtmosphereFrame::default().with_sky_kind(kind));
                for _ in 0..8 {
                    app.update();
                }
                let colour = pixel(&app, &image);
                if distance.is_some() {
                    assert!(
                        colour[1] > 200 && colour[0] < 20 && colour[2] < 20,
                        "{msaa:?}/{kind:?} sky overwrote the world at {distance:?}: {colour:?}"
                    );
                } else {
                    assert!(
                        colour[..3] != [255, 0, 255],
                        "{msaa:?}/{kind:?} stock sky left clear samples unfilled: {colour:?}"
                    );
                }
            }
        }
    }
}
