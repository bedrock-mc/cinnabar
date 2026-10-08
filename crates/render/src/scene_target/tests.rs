use super::*;

/// Requests a headless adapter and initializes the task pool used by RenderContext::finish.
fn fixture() -> Option<(RenderDevice, wgpu::Queue, wgpu::Adapter)> {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = match bevy::tasks::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!("missing fixture: headless GPU for shared scene attachment tests");
            return None;
        }
        Err(error) => panic!("shared scene adapter: {error}"),
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features()
            & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        ..Default::default()
    }))
    .expect("shared scene device");
    Some((RenderDevice::from(device), queue, adapter))
}

/// Keeps fixture readbacks bounded while exercising multiple fragments and samples.
fn size() -> Extent3d {
    Extent3d {
        width: 2,
        height: 2,
        depth_or_array_layers: 1,
    }
}

#[test]
fn shared_colour_retains_samples_until_one_discarding_resolve() {
    let Some((device, _, adapter)) = fixture() else {
        return;
    };
    let format = TextureFormat::Rgba8UnormSrgb;
    for samples in [1, 2, 4, 8] {
        if !adapter
            .get_texture_format_features(format)
            .flags
            .sample_count_supported(samples)
        {
            eprintln!("missing fixture: shared scene {samples}x MSAA format support");
            continue;
        }
        let scene = SceneTarget::new(&device, size(), format, samples);
        let resolved = SceneTarget::new(&device, size(), format, 1);
        let mut resolves = 0;
        for encoded in [false, true, false, true, false] {
            let attachment = scene.attachment(encoded, LoadOp::Load, None);
            assert!(attachment.resolve_target.is_none());
            assert_eq!(attachment.ops.store, StoreOp::Store);
            assert_eq!(scene.texture.sample_count(), samples);
            resolves += usize::from(attachment.resolve_target.is_some());
        }
        if samples > 1 {
            let final_attachment =
                scene.resolve_attachment(resolved.color_view(false), StoreOp::Discard);
            resolves += usize::from(final_attachment.resolve_target.is_some());
            assert_eq!(final_attachment.ops.store, StoreOp::Discard);
            assert_eq!(resolves, 1);
            assert_eq!(scene.texture.usage(), TextureUsages::RENDER_ATTACHMENT);
            assert!(!scene.resolved.load(Ordering::Relaxed));
            let folded = scene.final_attachment(resolved.color_view(false));
            assert!(folded.resolve_target.is_some());
            assert_eq!(folded.ops.store, StoreOp::Discard);
            assert!(scene.resolved.load(Ordering::Relaxed));
            let identity = scene.texture.id();
            scene.begin_frame();
            assert!(!scene.resolved.load(Ordering::Relaxed));
            assert_eq!(scene.texture.id(), identity);
        } else {
            assert_eq!(resolves, 0);
            assert_eq!(
                scene.texture.usage(),
                TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC | TextureUsages::COPY_DST
            );
        }
        assert!(scene.matches(size(), format, samples));
        assert!(!scene.matches(size(), format, if samples == 1 { 4 } else { 1 }));
        assert!(!scene.matches(size(), TextureFormat::Rgba16Float, samples));
        assert!(!scene.matches(Extent3d { width: 4, ..size() }, format, samples));
    }
}

/// Uses different opaque sample colours and a hand covering one sample to expose flattening.
fn pipeline(
    device: &RenderDevice,
    shader: &wgpu::ShaderModule,
    format: TextureFormat,
    samples: u32,
    fragment: &str,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device
        .wgpu_device()
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shared scene coverage fixture"),
            layout: None,
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some(fragment),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: samples,
                ..Default::default()
            },
            multiview: None,
            cache: None,
        })
}

/// Reads the first resolved texel after all production-format attachment transitions.
fn read_pixel(
    device: &RenderDevice,
    queue: &wgpu::Queue,
    mut context: RenderContext,
    texture: &Texture,
) -> [u8; 4] {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shared scene coverage readback"),
        size: 512,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    context.command_encoder().copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(2),
            },
        },
        size(),
    );
    queue.submit(context.finish().0);
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    buffer.slice(..).get_mapped_range()[..4].try_into().unwrap()
}

/// Deliberately flattens the opaque samples so the differential fixture can detect lost coverage.
fn reconstruct_coverage(device: &RenderDevice, context: &mut RenderContext, scene: &SceneTarget) {
    let format = TextureFormat::Rgba8UnormSrgb;
    let resolved = device.create_texture(&TextureDescriptor {
        label: Some("coverage reconstruction control"),
        size: size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = resolved.create_view(&default());
    {
        let attachments = [Some(scene.resolve_attachment(&view, StoreOp::Store))];
        context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("coverage reconstruction control resolve"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
    }
    let gpu = device.wgpu_device();
    let shader = gpu.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("coverage reconstruction control"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    return vec4f(vec2f(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - vec2f(1.0), 0.0, 1.0);
}
@fragment fn copy(@builtin(position) position: vec4f) -> @location(0) vec4f {
    return textureLoad(source, vec2i(position.xy), 0);
}
"#
            .into(),
        ),
    });
    let pipeline = pipeline(
        device,
        &shader,
        format,
        scene.texture.sample_count(),
        "copy",
        None,
    );
    let binding = gpu.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("coverage reconstruction control"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&view),
        }],
    });
    let attachments = [Some(scene.attachment(false, LoadOp::Load, None))];
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&RenderPassDescriptor {
            label: Some("coverage reconstruction control draw"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &binding, &[]);
    pass.draw(0..3, 0..1);
}

/// Renders the same geometry with optional resolved-colour reconstruction as a differential control.
fn coverage_pixel(
    device: &RenderDevice,
    queue: &wgpu::Queue,
    samples: u32,
    reconstruct: bool,
) -> [u8; 4] {
    let format = TextureFormat::Rgba8UnormSrgb;
    let shader = device
        .wgpu_device()
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shared scene coverage fixture"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    return vec4f(vec2f(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - vec2f(1.0), 0.0, 1.0);
}
@fragment fn opaque(@builtin(sample_index) sample: u32) -> @location(0) vec4f {
    return select(vec4f(1.0, 0.0, 0.0, 1.0), vec4f(0.0, 0.0, 1.0, 1.0), sample == 0u);
}
@fragment fn transparent() -> @location(0) vec4f { return vec4f(0.0, 1.0, 0.0, 0.5); }
struct Hand { @location(0) colour: vec4f, @builtin(sample_mask) coverage: u32 }
@fragment fn hand() -> Hand { return Hand(vec4f(1.0), 1u); }
"#
                .into(),
            ),
        });
    let scene = SceneTarget::new(device, size(), format, samples);
    let resolved = SceneTarget::new(device, size(), format, 1);
    let opaque = pipeline(device, &shader, format, samples, "opaque", None);
    let transparent = pipeline(
        device,
        &shader,
        format.remove_srgb_suffix(),
        samples,
        "transparent",
        Some(wgpu::BlendState::ALPHA_BLENDING),
    );
    let hand = pipeline(device, &shader, format, samples, "hand", None);
    let mut context = RenderContext::new(device.clone(), None);
    for (index, (encoded, pipeline)) in [(false, &opaque), (true, &transparent), (false, &hand)]
        .into_iter()
        .enumerate()
    {
        if reconstruct && index == 1 && samples > 1 {
            reconstruct_coverage(device, &mut context, &scene);
        }
        let attachments = [Some(scene.attachment(encoded, LoadOp::Load, None))];
        let mut pass = context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("shared scene fixture geometry"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        pass.set_pipeline(pipeline);
        pass.draw(0..3, 0..1);
    }
    let output = if samples > 1 {
        let attachments = [Some(
            scene.resolve_attachment(resolved.color_view(false), StoreOp::Discard),
        )];
        context
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("shared scene fixture final resolve"),
                color_attachments: &attachments,
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        &resolved.texture
    } else {
        &scene.texture
    };
    read_pixel(device, queue, context, output)
}

#[test]
fn encoded_transparency_and_hand_preserve_individual_opaque_samples() {
    let Some((device, queue, adapter)) = fixture() else {
        return;
    };
    for samples in [1, 2, 4, 8] {
        if !adapter
            .get_texture_format_features(TextureFormat::Rgba8UnormSrgb)
            .flags
            .sample_count_supported(samples)
        {
            eprintln!("missing fixture: shared scene {samples}x MSAA format support");
            continue;
        }
        let pixel = coverage_pixel(&device, &queue, samples, false);
        if samples == 1 {
            assert_eq!(pixel, [255; 4]);
        } else {
            assert!(
                pixel[0].abs_diff(pixel[1]) <= 1,
                "sample coverage was flattened at {samples}x: {pixel:?}"
            );
            assert!(
                pixel[0] > pixel[2] && pixel[2] > 0,
                "hand coverage must affect exactly one sample: {pixel:?}"
            );
            assert_eq!(pixel[3], 255);
        }
    }
}

#[test]
fn coverage_fixture_detects_resolved_colour_reconstruction() {
    let Some((device, queue, adapter)) = fixture() else {
        return;
    };
    for samples in [2, 4, 8] {
        if !adapter
            .get_texture_format_features(TextureFormat::Rgba8UnormSrgb)
            .flags
            .sample_count_supported(samples)
        {
            eprintln!("missing fixture: reconstructed scene {samples}x MSAA format support");
            continue;
        }
        let pixel = coverage_pixel(&device, &queue, samples, true);
        assert!(
            pixel[0] + 1 < pixel[1],
            "fixture must distinguish flattened opaque samples: {pixel:?}"
        );
    }
}

#[test]
fn main_attachment_nodes_preserve_graph_dependencies() {
    use bevy::render::render_graph::EmptyNode;
    let mut world = World::new();
    let mut core = RenderGraph::default();
    for label in [
        Node3d::MainOpaquePass,
        Node3d::MainTransmissivePass,
        Node3d::MainTransparentPass,
        Node3d::EndMainPass,
    ] {
        core.add_node(label, EmptyNode);
    }
    core.add_node_edges((
        Node3d::MainOpaquePass,
        Node3d::MainTransmissivePass,
        Node3d::MainTransparentPass,
        Node3d::EndMainPass,
    ));
    let mut graph = RenderGraph::default();
    graph.add_sub_graph(Core3d, core);
    world.insert_resource(graph);
    install_graph(&mut world);
    let graph = world
        .resource::<RenderGraph>()
        .get_sub_graph(Core3d)
        .unwrap();
    let opaque = graph.get_node_state(Node3d::MainOpaquePass).unwrap();
    assert!(
        opaque
            .node::<ViewNodeRunner<nodes::SceneOpaquePass>>()
            .is_ok()
    );
    assert_eq!(opaque.edges.output_edges().len(), 1);
    let finish = graph.get_node_state(Node3d::EndMainPass).unwrap();
    assert!(finish.node::<ViewNodeRunner<nodes::SceneFinish>>().is_ok());
    assert_eq!(finish.edges.input_edges().len(), 1);
}

/// An MSAA view keeps exactly one multisampled colour texture: the shared scene attachment.
#[test]
fn msaa_view_allocates_one_multisampled_colour_target() {
    use bevy::{
        camera::{
            CameraOutputMode, ClearColorConfig, MsaaWriteback, NormalizedRenderTarget, RenderTarget,
        },
        render::{
            camera::ExtractedCamera,
            render_graph::RenderSubGraph,
            texture::{OutputColorAttachment, TextureCache},
            view::{
                ExtractedView, RetainedViewEntity, ViewTargetAttachments, prepare_view_targets,
            },
        },
    };
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..Default::default()
        },
        ..Default::default()
    });
    let adapter = bevy::tasks::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, _) = bevy::tasks::block_on(adapter.request_device(&Default::default())).unwrap();
    let device = RenderDevice::from(device);
    let mut world = World::new();
    let world = &mut world;
    world.insert_resource(device.clone());
    let output = device.create_texture(&TextureDescriptor {
        label: Some("scene sample ownership output"),
        size: size(),
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Bgra8UnormSrgb,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let target: NormalizedRenderTarget =
        RenderTarget::Window(bevy::window::WindowRef::Entity(Entity::PLACEHOLDER))
            .normalize(None)
            .unwrap();
    let mut attachments = ViewTargetAttachments::default();
    attachments.insert(
        target.clone(),
        OutputColorAttachment::new(output.create_view(&Default::default()), output.format()),
    );
    world.insert_resource(attachments);
    world.insert_resource(TextureCache::default());
    world.insert_resource(ClearColor::default());
    world.init_resource::<WithheldSamples>();
    let view = world
        .spawn((
            Camera3d::default(),
            ExtractedCamera {
                target: Some(target),
                physical_viewport_size: Some(UVec2::new(size().width, size().height)),
                physical_target_size: Some(UVec2::new(size().width, size().height)),
                viewport: None,
                render_graph: Core3d.intern(),
                order: 0,
                output_mode: CameraOutputMode::default(),
                msaa_writeback: MsaaWriteback::default(),
                clear_color: ClearColorConfig::default(),
                sorted_camera_index_for_target: 0,
                exposure: 1.0,
                hdr: false,
            },
            ExtractedView {
                retained_view_entity: RetainedViewEntity::new(Entity::PLACEHOLDER.into(), None, 1),
                clip_from_view: Mat4::IDENTITY,
                world_from_view: GlobalTransform::default(),
                clip_from_world: None,
                hdr: false,
                viewport: UVec4::new(0, 0, size().width, size().height),
                color_grading: Default::default(),
                invert_culling: false,
            },
            CameraMainTextureUsages::default(),
            Msaa::Sample4,
        ))
        .id();
    let mut schedule = Render::base_schedule();
    schedule.add_systems((
        prepare_view_targets.in_set(RenderSystems::ManageViews),
        render_systems(),
    ));
    schedule.run(world);
    let view = world.entity(view);
    assert_eq!(view.get::<Msaa>(), Some(&Msaa::Sample4));
    assert!(
        view.get::<ViewTarget>()
            .unwrap()
            .sampled_main_texture()
            .is_none()
    );
    assert_eq!(view.get::<SceneTarget>().unwrap().texture.sample_count(), 4);
}
