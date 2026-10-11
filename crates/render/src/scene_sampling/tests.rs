use super::*;
use bevy::render::texture::CachedTexture;

/// Requests the format capabilities used by the live renderer without opening a window.
pub(crate) fn fixture() -> Option<(RenderDevice, wgpu::Queue, wgpu::Adapter)> {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = match bevy::tasks::block_on(instance.request_adapter(&Default::default())) {
        Ok(adapter) => adapter,
        Err(wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!("skipping scene sampling: missing native GPU fixture");
            return None;
        }
        Err(error) => panic!("scene sampling adapter: {error}"),
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: adapter.features()
            & wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        ..Default::default()
    }))
    .unwrap();
    Some((RenderDevice::from(device), queue, adapter))
}

/// Creates the bounded fixture attachments with exactly their required usages.
pub(crate) fn texture(
    device: &RenderDevice,
    format: wgpu::TextureFormat,
    samples: u32,
    usage: wgpu::TextureUsages,
) -> Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene sampling fixture"),
        size: wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    })
}

/// Reads one pixel after submitting the production commands.
pub(crate) fn pixel(
    device: &RenderDevice,
    queue: &wgpu::Queue,
    mut commands: Vec<wgpu::CommandBuffer>,
    texture: &Texture,
) -> [u8; 4] {
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scene sampling readback"),
        size: 512,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            aspect: if texture.format().is_depth_stencil_format() {
                wgpu::TextureAspect::DepthOnly
            } else {
                wgpu::TextureAspect::All
            },
            ..texture.as_image_copy()
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(2),
            },
        },
        wgpu::Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
    );
    commands.push(encoder.finish());
    queue.submit(commands);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |result| result.unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    readback.slice(..).get_mapped_range()[..4]
        .try_into()
        .unwrap()
}

/// Writes distinct sample depths; one uncovered sample is the reverse-Z clear value.
fn fill_depth(context: &mut RenderContext, depth: &ViewDepthTexture, uncovered: bool) {
    let device = context.render_device().wgpu_device();
    let source = format!(
        "\
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {{
    return vec4(vec2(f32((i << 1u) & 2u), f32(i & 2u)) * 2.0 - vec2(1.0), 0.0, 1.0);
}}
@fragment fn fragment(@builtin(sample_index) sample: u32) -> @builtin(frag_depth) f32 {{
    return select(f32(sample + 1u) * 0.1, 0.0, {} && sample == 0u);
}}",
        uncovered
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sample coverage fixture"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[],
        }),
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: depth.texture.sample_count(),
            ..Default::default()
        },
        multiview_mask: None,
        cache: None,
    });
    let mut pass = context
        .command_encoder()
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sample coverage fixture"),
            color_attachments: &[],
            depth_stencil_attachment: Some(depth.get_attachment(wgpu::StoreOp::Store)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
    pass.set_pipeline(&pipeline);
    pass.draw(0..3, 0..1);
}

/// Runs the production Hi-Z seed directly on the same depth consumed by post effects.
fn hiz_seed(context: &mut RenderContext, depth: &ViewDepthTexture) -> Texture {
    let device = context.render_device();
    let destination = texture(
        device,
        wgpu::TextureFormat::R32Float,
        1,
        wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
    );
    let view = destination.create_view(&Default::default());
    let device = device.wgpu_device();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production Hi-Z fixture"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../chunk/gpu_cull/hiz.wgsl").into()),
    });
    let multi = depth.texture.sample_count() > 1;
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some(if multi {
            "hiz_seed_multisampled"
        } else {
            "hiz_seed"
        }),
        compilation_options: Default::default(),
        cache: None,
    });
    // The seed's source is the whole depth target and it builds every covering texel.
    let size = depth.texture.size();
    let bounds = wgpu::util::DeviceExt::create_buffer_init(
        device,
        &wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&[
                size.width - 1,
                size.height - 1,
                size.width.div_ceil(2),
                size.height.div_ceil(2),
            ]),
            usage: wgpu::BufferUsages::UNIFORM,
        },
    );
    let binding = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: u32::from(multi),
                resource: wgpu::BindingResource::TextureView(depth.view()),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: bounds.as_entire_binding(),
            },
        ],
    });
    let mut pass = context
        .command_encoder()
        .begin_compute_pass(&Default::default());
    pass.set_pipeline(&pipeline);
    pass.set_bind_group(0, &binding, &[]);
    pass.dispatch_workgroups(1, 1, 1);
    drop(pass);
    destination
}

#[test]
fn scene_depth_and_hiz_preserve_nearest_and_conservative_sample_coverage() {
    let Some((device, queue, adapter)) = fixture() else {
        return;
    };
    let format = wgpu::TextureFormat::Depth32Float;
    for samples in [1, 2, 4, 8] {
        if !adapter
            .get_texture_format_features(format)
            .flags
            .sample_count_supported(samples)
        {
            eprintln!("skipping depth resolve {samples} samples: missing format support");
            continue;
        }
        let texture = texture(
            &device,
            format,
            samples,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        );
        let depth = ViewDepthTexture::new(
            CachedTexture {
                default_view: texture.create_view(&Default::default()),
                texture,
            },
            Some(0.0),
        );
        let resolved = ResolvedDepth::new(&device, &depth, RuntimeStage::GpuPost);
        assert!(resolved.matches(&depth));
        for uncovered in [false, true] {
            let (_, commands) =
                crate::render_test_support::record(&mut World::new(), &device, |world, context| {
                    fill_depth(context, &depth, uncovered);
                    resolved.draw(context, world, None);
                });
            let expected = if samples == 1 && uncovered {
                0.0
            } else {
                samples as f32 * 0.1
            };
            assert!(
                (f32::from_le_bytes(pixel(&device, &queue, commands, &resolved._texture))
                    - expected)
                    .abs()
                    < 1e-6
            );
            let (pyramid, commands) =
                crate::render_test_support::record(&mut World::new(), &device, |_, context| {
                    hiz_seed(context, &depth)
                });
            let farthest = f32::from_le_bytes(pixel(&device, &queue, commands, &pyramid));
            assert!((farthest - if uncovered { 0.0 } else { 0.1 }).abs() < 1e-6);
        }
    }
}

#[test]
fn scene_depth_shader_validates() {
    let module = naga::front::wgsl::parse_str(include_str!("../scene_depth.wgsl")).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
}
