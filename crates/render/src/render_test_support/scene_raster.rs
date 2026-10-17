//! Shared native offscreen scene fixtures for rendering comparisons.

use bevy::{
    asset::{AssetPlugin, Assets},
    camera::{CameraPlugin, RenderTarget},
    core_pipeline::{Core3d, CorePipelinePlugin, tonemapping::Tonemapping},
    image::ImagePlugin,
    mesh::MeshPlugin,
    prelude::*,
    render::{
        RenderApp, RenderPlugin,
        render_asset::RenderAssets,
        render_resource::{
            BufferUsages, ColorWrites, CompareFunction, Extent3d, StoreOp, TextureFormat,
            TextureUsages,
        },
        renderer::{
            RenderAdapter, RenderAdapterInfo, RenderContext, RenderDevice, RenderInstance,
            RenderQueue,
        },
        settings::RenderCreation,
        texture::GpuImage,
        view::{ViewDepthStencilTexture, ViewTarget},
    },
    window::WindowPlugin,
};

pub(crate) const SIDE: u32 = 64;

#[derive(Resource)]
pub(crate) struct FixturePipelines {
    pub checker: wgpu::RenderPipeline,
    pub sloped_checker: wgpu::RenderPipeline,
    pub silhouette: wgpu::RenderPipeline,
    pub scene: u8,
}

/// Draws known colour and depth patterns into the shared scene attachment.
fn fixture_opaque(
    world: &World,
    query: bevy::render::renderer::ViewQuery<(
        &'static ViewTarget,
        &'static crate::scene_target::SceneTarget,
        &'static ViewDepthStencilTexture,
    )>,
    mut context: RenderContext,
) {
    let (target, scene, depth) = query.into_inner();
    let fixture = world.resource::<FixturePipelines>();
    let attachments = [Some(scene.color_attachment(target, false))];
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("depth SMAA raster fixture"),
            color_attachments: &attachments,
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(match fixture.scene {
        0 => &fixture.checker,
        1 => &fixture.sloped_checker,
        _ => &fixture.silhouette,
    });
    pass.draw(0..3, 0..1);
}

/// Requests a native adapter and its actual texture capabilities without creating a surface.
#[allow(clippy::print_stderr, reason = "explains missing native GPU fixtures")]
pub(crate) fn renderer() -> Option<RenderPlugin> {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = match bevy::tasks::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!(
                "skipping spatial_smaa_preserves_texels_and_filters_depth_silhouettes: missing native GPU fixture"
            );
            return None;
        }
        Err(error) => panic!("depth SMAA raster adapter: {error}"),
    };
    let info = adapter.get_info();
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features()
            & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        required_limits: adapter.limits(),
        ..default()
    }))
    .expect("depth SMAA raster device");
    Some(RenderPlugin {
        render_creation: RenderCreation::manual(
            RenderDevice::from(device),
            RenderQueue::new(queue),
            RenderAdapterInfo::new(info),
            RenderAdapter::new(adapter),
            RenderInstance::new(instance),
        ),
        synchronous_pipeline_compilation: true,
        ..default()
    })
}

/// Two shader entries distinguish authored colour boundaries from a diagonal depth discontinuity.
pub(crate) fn fixture_pipeline(
    device: &RenderDevice,
    samples: u32,
    entry: &str,
) -> wgpu::RenderPipeline {
    fixture_pipeline_format(device, samples, render_scene::SCENE_COLOR_FORMAT, entry)
}

/// Uses the requested scene format to exercise both encoded colour and HDR filtering.
pub(crate) fn fixture_pipeline_format(
    device: &RenderDevice,
    samples: u32,
    format: TextureFormat,
    entry: &str,
) -> wgpu::RenderPipeline {
    let shader = device
        .wgpu_device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("depth SMAA geometry fixture"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4(corner * 2.0 - vec2(1.0), 0.0, 1.0);
}
struct Fragment { @location(0) color: vec4<f32>, @builtin(frag_depth) depth: f32 }
@fragment fn checker(@builtin(position) position: vec4<f32>) -> Fragment {
    let cell = vec2<u32>(position.xy) / vec2(8u);
    let odd = (cell.x + cell.y) % 2u != 0u;
    return Fragment(vec4(select(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), odd), 1.0), 0.5);
}
@fragment fn sloped_checker(@builtin(position) position: vec4<f32>) -> Fragment {
    let cell = vec2<u32>(position.xy) / vec2(8u);
    let odd = (cell.x + cell.y) % 2u != 0u;
    return Fragment(vec4(select(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), odd), 1.0), 0.1 + 0.002 * position.x);
}
@fragment fn silhouette(@builtin(position) position: vec4<f32>) -> Fragment {
    let front = position.x + position.y * 0.6 < 49.0;
    return Fragment(vec4(vec3(select(0.0, 1.0, front)), 1.0), select(0.2, 0.7, front));
}
@fragment fn covered_colour(
    @builtin(position) position: vec4<f32>,
    @builtin(sample_index) sample: u32,
) -> Fragment {
    let front = position.x + position.y * 0.6 + f32(sample) * 0.3 < 49.0;
    let colour = vec3(position.x / 63.0, position.y / 63.0, f32(sample) * 0.13 + 0.05);
    return Fragment(vec4(select(colour, colour.brg * 2.0, front), 0.37), select(0.2, 0.7, front));
}
"#
                .into(),
            ),
        });
    device
        .wgpu_device()
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("depth SMAA opaque fixture"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vertex"),
                compilation_options: default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(entry),
                compilation_options: default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::Always),
                stencil: default(),
                bias: default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..default()
            },
            multiview_mask: None,
            cache: None,
        })
}

/// Initializes the production scene with image output, known geometry and caller-supplied effects.
pub(crate) fn app_with_effects(
    render: RenderPlugin,
    setup: impl FnOnce(&mut App),
) -> (App, Entity, Handle<Image>) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(WindowPlugin {
            primary_window: None,
            ..default()
        })
        .add_plugins((AssetPlugin::default(), TransformPlugin))
        .add_plugins(render)
        .add_plugins((
            ImagePlugin::default(),
            MeshPlugin,
            CameraPlugin,
            CorePipelinePlugin,
        ));
    crate::scene_target::install(&mut app);
    crate::render_test_support::fail_on_render_error(&mut app);
    let mut output = Image::new_target_texture(SIDE, SIDE, TextureFormat::Rgba8Unorm, None);
    output.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let image = app.world_mut().resource_mut::<Assets<Image>>().add(output);
    let camera = app
        .world_mut()
        .spawn((
            Camera3d {
                depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT
                    | TextureUsages::TEXTURE_BINDING)
                    .into(),
                ..default()
            },
            Camera::default(),
            RenderTarget::Image(image.clone().into()),
            Msaa::Off,
            Transform::default(),
            Tonemapping::Linear,
        ))
        .id();
    setup(&mut app);
    app.finish();
    app.cleanup();
    let render = app.sub_app_mut(RenderApp).world_mut();
    crate::scene_target::install_graph(render);
    render.schedule_scope(Core3d, |world, schedule| {
        schedule
            .remove_systems_in_set(
                crate::scene_target::ScenePass::Opaque,
                world,
                bevy::ecs::schedule::ScheduleCleanupPolicy::RemoveSystemsOnly,
            )
            .unwrap();
        schedule.add_systems(fixture_opaque.in_set(crate::scene_target::ScenePass::Opaque));
        schedule.configure_sets(
            crate::ui_render::UiWorldLabel
                .after(crate::scene_target::ScenePass::Transparent)
                .before(crate::scene_target::ScenePass::Finish),
        );
    });
    (app, camera, image)
}

/// Advances complete rendering and waits for submitted native commands before readback.
pub(crate) fn frame(app: &mut App) {
    app.update();
    app.sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
}

/// Reads the last submitted frame without changing camera history through another update.
pub(crate) fn read_pixels(app: &App, image: &Handle<Image>) -> Vec<u8> {
    let world = app.sub_app(RenderApp).world();
    let device = world.resource::<RenderDevice>();
    let queue = world.resource::<RenderQueue>();
    let image = world
        .resource::<RenderAssets<GpuImage>>()
        .get(image)
        .expect("offscreen image prepared");
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("SMAA raster readback"),
        size: (SIDE * SIDE * 4) as u64,
        usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device.wgpu_device().create_command_encoder(&default());
    encoder.copy_texture_to_buffer(
        image.texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SIDE * 4),
                rows_per_image: Some(SIDE),
            },
        },
        Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    buffer
        .slice(..)
        .get_mapped_range()
        .expect("readback buffer is mapped")
        .to_vec()
}

/// Saves optional native fixture evidence outside the checkout when capture is requested.
pub(crate) fn capture(name: &str, samples: u32, pixels: &[u8]) {
    if std::env::var_os("CINNABAR_SMAA_RASTER_CAPTURE").as_deref()
        != Some(std::ffi::OsStr::new("1"))
    {
        return;
    }
    let root = std::path::Path::new("/private/tmp/sol-smaa");
    std::fs::create_dir_all(root).unwrap();
    image::save_buffer_with_format(
        root.join(format!("{name}-{samples}x.png")),
        pixels,
        SIDE,
        SIDE,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .unwrap();
}
