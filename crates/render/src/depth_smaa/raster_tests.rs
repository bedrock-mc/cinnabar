//! Native offscreen frames exercise depth edges, stock SMAA lookup tables and scene restoration.

use super::super::*;
use bevy::{
    asset::{AssetPlugin, Assets},
    camera::{CameraPlugin, RenderTarget},
    core_pipeline::{CorePipelinePlugin, tonemapping::Tonemapping},
    ecs::query::QueryItem,
    image::{BevyDefault, ImagePlugin},
    mesh::MeshPlugin,
    render::{
        RenderPlugin,
        render_asset::RenderAssets,
        render_graph::{EmptyNode, NodeRunError, RenderGraphContext, ViewNode},
        renderer::{
            RenderAdapter, RenderAdapterInfo, RenderContext, RenderInstance, RenderQueue,
            WgpuWrapper,
        },
        settings::RenderCreation,
        texture::GpuImage,
    },
    window::WindowPlugin,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const SIDE: u32 = 64;

#[derive(Resource)]
struct FixturePipelines {
    checker: wgpu::RenderPipeline,
    sloped_checker: wgpu::RenderPipeline,
    silhouette: wgpu::RenderPipeline,
    scene: u8,
}

struct FixtureOpaque;

impl ViewNode for FixtureOpaque {
    type ViewQuery = (
        &'static ViewTarget,
        &'static crate::scene_target::SceneTarget,
        &'static ViewDepthTexture,
    );

    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (target, scene, depth): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
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
            });
        pass.set_pipeline(match fixture.scene {
            0 => &fixture.checker,
            1 => &fixture.sloped_checker,
            _ => &fixture.silhouette,
        });
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

/// Requests a native adapter and its actual texture capabilities without creating a surface.
fn renderer() -> Option<RenderPlugin> {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
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
            RenderQueue(Arc::new(WgpuWrapper::new(queue))),
            RenderAdapterInfo(WgpuWrapper::new(info)),
            RenderAdapter(Arc::new(WgpuWrapper::new(adapter))),
            RenderInstance(Arc::new(WgpuWrapper::new(instance))),
        ),
        synchronous_pipeline_compilation: true,
        ..default()
    })
}

/// Two shader entries distinguish authored colour boundaries from a diagonal depth discontinuity.
fn fixture_pipeline(device: &RenderDevice, samples: u32, entry: &str) -> wgpu::RenderPipeline {
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
                    format: TextureFormat::bevy_default(),
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
            }),
            primitive: default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: CompareFunction::Always,
                stencil: default(),
                bias: default(),
            }),
            multisample: wgpu::MultisampleState {
                count: samples,
                ..default()
            },
            multiview: None,
            cache: None,
        })
}

/// Initializes the production render graph with image output and a known opaque fixture.
fn app(render: RenderPlugin) -> (App, Entity, Handle<Image>) {
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
    app.add_plugins(DepthSmaaPlugin);
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
            Tonemapping::None,
        ))
        .id();
    app.finish();
    app.cleanup();
    let render = app.sub_app_mut(RenderApp).world_mut();
    crate::scene_target::install_graph(render);
    let fixture = ViewNodeRunner::new(FixtureOpaque, render);
    let mut graphs = render.resource_mut::<RenderGraph>();
    let core = graphs.get_sub_graph_mut(Core3d).unwrap();
    core.get_node_state_mut(Node3d::MainOpaquePass)
        .unwrap()
        .node = Box::new(fixture);
    core.add_node(crate::ui_render::UiWorldLabel, EmptyNode);
    core.add_node_edges((
        Node3d::MainTransparentPass,
        crate::ui_render::UiWorldLabel,
        Node3d::EndMainPass,
    ));
    (app, camera, image)
}

/// Advances complete rendering and waits for submitted native commands before readback.
fn frame(app: &mut App) {
    app.update();
    app.sub_app(RenderApp)
        .world()
        .resource::<RenderDevice>()
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
}

/// Waits for real SMAA lookup images, bindings and all production pipelines to become usable.
fn ready(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        frame(app);
        let world = app.sub_app_mut(RenderApp).world_mut();
        let prepared = world
            .query::<(&DepthSmaaView, &bevy::anti_alias::smaa::SmaaBindGroups)>()
            .iter(world)
            .next()
            .map(|(view, _)| view.ids);
        if let Some(ids) = prepared {
            let cache = world.resource::<PipelineCache>();
            if [ids.edge, ids.weights, ids.blend, ids.restore]
                .into_iter()
                .all(|id| cache.get_render_pipeline(id).is_some())
            {
                frame(app);
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "native SMAA pipelines and lookup tables did not become ready"
        );
        std::thread::yield_now();
    }
}

/// Reads the final output image after all scene and SMAA graph nodes have completed.
fn pixels(app: &mut App, image: &Handle<Image>) -> Vec<u8> {
    frame(app);
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
    buffer.slice(..).get_mapped_range().to_vec()
}

/// Saves optional native fixture evidence outside the checkout when capture is requested.
fn capture(name: &str, samples: u32, pixels: &[u8]) {
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

#[test]
fn spatial_smaa_preserves_texels_and_filters_depth_silhouettes() {
    let Some(render) = renderer() else {
        return;
    };
    let (mut app, camera, image) = app(render);
    for msaa in [Msaa::Off, Msaa::Sample4] {
        let world = app.sub_app(RenderApp).world();
        let adapter = world.resource::<RenderAdapter>();
        if [
            TextureFormat::bevy_default(),
            bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
        ]
        .into_iter()
        .any(|format| {
            !adapter
                .get_texture_format_features(format)
                .flags
                .sample_count_supported(msaa.samples())
        }) {
            eprintln!(
                "skipping native SMAA {}x MSAA: missing attachment sample support",
                msaa.samples()
            );
            continue;
        }
        *app.world_mut().get_mut::<Msaa>(camera).unwrap() = msaa;
        let device = app.sub_app(RenderApp).world().resource::<RenderDevice>();
        let fixture = FixturePipelines {
            checker: fixture_pipeline(device, msaa.samples(), "checker"),
            sloped_checker: fixture_pipeline(device, msaa.samples(), "sloped_checker"),
            silhouette: fixture_pipeline(device, msaa.samples(), "silhouette"),
            scene: 0,
        };
        app.sub_app_mut(RenderApp)
            .world_mut()
            .insert_resource(fixture);
        app.world_mut().entity_mut(camera).insert(Smaa::default());
        ready(&mut app);
        app.world_mut().entity_mut(camera).remove::<Smaa>();
        frame(&mut app);
        let checker_off = pixels(&mut app, &image);
        assert!(
            checker_off
                .chunks_exact(4)
                .any(|pixel| pixel[0] > 240 && pixel[1] < 10)
        );
        assert!(
            checker_off
                .chunks_exact(4)
                .any(|pixel| pixel[1] > 240 && pixel[0] < 10)
        );
        app.world_mut().entity_mut(camera).insert(Smaa::default());
        ready(&mut app);
        let checker_on = pixels(&mut app, &image);
        capture("checker-off", msaa.samples(), &checker_off);
        capture("checker-smaa", msaa.samples(), &checker_on);
        assert_eq!(
            checker_off,
            checker_on,
            "flat-depth authored texel boundaries changed at {}x MSAA",
            msaa.samples()
        );
        app.sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<FixturePipelines>()
            .scene = 1;
        app.world_mut().entity_mut(camera).remove::<Smaa>();
        frame(&mut app);
        let sloped_off = pixels(&mut app, &image);
        app.world_mut().entity_mut(camera).insert(Smaa::default());
        ready(&mut app);
        assert_eq!(
            sloped_off,
            pixels(&mut app, &image),
            "continuous sloped depth smeared authored texel boundaries at {}x MSAA",
            msaa.samples()
        );
        app.sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<FixturePipelines>()
            .scene = 2;
        app.world_mut().entity_mut(camera).remove::<Smaa>();
        frame(&mut app);
        let silhouette_off = pixels(&mut app, &image);
        app.world_mut().entity_mut(camera).insert(Smaa::default());
        ready(&mut app);
        let silhouette_on = pixels(&mut app, &image);
        capture("silhouette-off", msaa.samples(), &silhouette_off);
        capture("silhouette-smaa", msaa.samples(), &silhouette_on);
        let changed = silhouette_off
            .chunks_exact(4)
            .zip(silhouette_on.chunks_exact(4))
            .filter(|(before, after)| before != after)
            .count();
        assert!(
            changed > 0,
            "SMAA did not filter the diagonal depth silhouette at {}x MSAA",
            msaa.samples()
        );
        assert!(
            silhouette_on
                .chunks_exact(4)
                .any(|pixel| pixel[0] > 0 && pixel[0] < 255),
            "SMAA silhouette lacks intermediate coverage"
        );
    }
}
