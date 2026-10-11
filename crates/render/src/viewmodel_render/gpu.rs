//! Retained GPU resources, uploads and pipeline variants for the camera-local hand.

use super::*;

#[derive(Default, Resource)]
pub(super) struct HandDrawn(pub(super) Mutex<Option<ViewmodelToken>>);
pub(super) struct PixelGpu {
    pub(super) _texture: Texture,
    pub(super) view: TextureView,
    pub(super) identity: [u8; 32],
    pub(super) geometry: [u8; 32],
    pub(super) pixels: std::sync::Arc<[u8]>,
}
pub(super) struct DepthGpu {
    pub(super) _texture: Texture,
    pub(super) view: TextureView,
    pub(super) size: [u32; 2],
    pub(super) samples: u32,
}
#[derive(Resource)]
pub(super) struct HandGpu {
    pub(super) device: wgpu::Device,
    pub(super) device_observation: DeviceObservation,
    pub(super) projection: Buffer,
    pub(super) sampler: Sampler,
    pub(super) layout: BindGroupLayoutDescriptor,
    pub(super) vertices: Option<Buffer>,
    pub(super) geometry: Option<[u8; 32]>,
    pub(super) skin: Option<PixelGpu>,
    pub(super) depth: Option<DepthGpu>,
    pub(super) bind_group: Option<BindGroup>,
    pub(super) token: Option<ViewmodelToken>,
    pub(super) pipeline: Option<CachedRenderPipelineId>,
    pub(super) pipeline_variants: [Option<CachedRenderPipelineId>; 8],
    pub(super) vertex_count: u32,
}

/// Allocates device-owned hand resources and resets publication completion state.
pub(super) fn init_gpu(
    mut commands: Commands,
    device: Res<RenderDevice>,
    tick: SystemChangeTick,
    gate: Res<ViewmodelCompletionGate>,
    drawn: Res<HandDrawn>,
    coverage: Option<Res<crate::ui_render::UiHandCoverage>>,
) {
    gate.select(None);
    *drawn.0.lock().expect("hand drawn lock") = None;
    if let Some(coverage) = coverage {
        coverage.clear();
    }
    commands.insert_resource(HandGpu {
        device: device.wgpu_device().clone(),
        device_observation: DeviceObservation::new(tick.this_run()),
        projection: device.create_buffer(&BufferDescriptor {
            label: Some("neutral hand projection"),
            size: 64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("neutral binary-alpha hand nearest"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            ..default()
        }),
        layout: hand_layout(),
        vertices: None,
        geometry: None,
        skin: None,
        depth: None,
        bind_group: None,
        token: None,
        pipeline: None,
        pipeline_variants: [None; 8],
        vertex_count: 0,
    });
}

#[derive(SystemParam)]
pub(super) struct PrepareViewmodel<'w, 's> {
    pub(super) scene: Res<'w, ViewmodelScene>,
    pub(super) background: Option<Res<'w, crate::panorama::PanoramaScene>>,
    pub(super) device: Res<'w, RenderDevice>,
    pub(super) adapter: Res<'w, RenderAdapter>,
    pub(super) queue: Res<'w, RenderQueue>,
    pub(super) cache: Res<'w, PipelineCache>,
    pub(super) gpu: ResMut<'w, HandGpu>,
    pub(super) gate: Res<'w, ViewmodelCompletionGate>,
    pub(super) drawn: Res<'w, HandDrawn>,
    pub(super) views: Query<
        'w,
        's,
        (
            &'static MainEntity,
            &'static ExtractedView,
            &'static bevy::render::camera::ExtractedCamera,
            &'static Msaa,
        ),
    >,
    pub(super) coverage: Option<Res<'w, crate::ui_render::UiHandCoverage>>,
    pub(super) tick: SystemChangeTick,
}

/// Admits the current hand frame and retains unchanged geometry, textures and pipelines.
pub(super) fn prepare(params: PrepareViewmodel) {
    let PrepareViewmodel {
        scene,
        background,
        device,
        adapter,
        queue,
        cache,
        mut gpu,
        gate,
        drawn,
        views,
        coverage,
        tick,
    } = params;
    let same_device = &gpu.device == device.wgpu_device();
    let device_valid =
        gpu.device_observation
            .observe(device.last_changed(), tick.this_run(), same_device);
    if let Some(coverage) = coverage {
        coverage.clear();
    }
    *drawn.0.lock().expect("hand drawn lock") = None;
    if !device_valid {
        ViewmodelCompletionGate::observe_stage(2, 1, scene.frame.as_ref().map(|frame| frame.token));
        if let Some(frame) = &scene.frame {
            gate.reject(frame.token);
        }
        invalidate_hand_resources(&mut gpu);
        return;
    }
    if background.is_some_and(|background| !background.game_visible()) {
        deactivate_hand(&mut gpu);
        return;
    }
    let Some(frame) = &scene.frame else {
        ViewmodelCompletionGate::observe_stage(2, 2, None);
        deactivate_hand(&mut gpu);
        return;
    };
    let token = frame.token;
    if frame.fallback.is_none() {
        ViewmodelCompletionGate::observe_stage(2, 3, Some(token));
        gate.reject(token);
        gpu.token = None;
        return;
    }
    if gpu.skin.as_ref().is_some_and(|old| {
        old.identity == token.skin
            && !std::sync::Arc::ptr_eq(&old.pixels, &frame.skin.rgba8)
            && old.pixels != frame.skin.rgba8
    }) {
        ViewmodelCompletionGate::observe_stage(2, 4, Some(token));
        gate.reject(token);
        gpu.token = None;
        return;
    }
    let view_valid = views.iter().any(|(owner, view, camera, msaa)| {
        owner.id() == token.owner
            && camera.hdr == token.hdr
            && msaa.samples() == token.samples
            && view.viewport == UVec4::new(0, 0, token.viewport[0], token.viewport[1])
    });
    let features = adapter.get_texture_format_features(TextureFormat::Depth32Float);
    if !view_valid
        || viewmodel_depth_bytes(token.viewport, token.samples).is_none()
        || token
            .viewport
            .iter()
            .any(|v| *v > device.limits().max_texture_dimension_2d)
        || !features.flags.sample_count_supported(token.samples)
        || !features
            .allowed_usages
            .contains(TextureUsages::RENDER_ATTACHMENT)
    {
        ViewmodelCompletionGate::observe_stage(2, 5, Some(token));
        gate.reject(token);
        gpu.token = None;
        gpu.depth = None;
        return;
    }
    if gpu.token == Some(token) {
        ViewmodelCompletionGate::observe_stage(2, 0, Some(token));
        return;
    }
    if gpu.geometry != Some(token.geometry) {
        gpu.vertices = None;
        gpu.vertices = Some(device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("validated neutral arm and sleeve"),
            contents: bytemuck::cast_slice(&frame.geometry.vertices),
            usage: BufferUsages::VERTEX,
        }));
        gpu.geometry = Some(token.geometry);
        gpu.vertex_count = frame.geometry.vertices.len() as u32;
    }
    if gpu
        .skin
        .as_ref()
        .is_none_or(|skin| skin.identity != token.skin || skin.geometry != token.geometry)
    {
        gpu.bind_group = None;
        gpu.skin = None;
        let texture = device.create_texture_with_data(
            &queue,
            &TextureDescriptor {
                label: Some("validated neutral hand skin"),
                size: Extent3d {
                    width: crate::viewmodel::VIEWMODEL_TEXTURE_SIDE,
                    height: crate::viewmodel::VIEWMODEL_TEXTURE_SIDE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &frame.skin.rgba8,
        );
        let view = texture.create_view(&TextureViewDescriptor::default());
        gpu.skin = Some(PixelGpu {
            _texture: texture,
            view,
            identity: token.skin,
            geometry: token.geometry,
            pixels: std::sync::Arc::clone(&frame.skin.rgba8),
        });
    }
    if gpu
        .depth
        .as_ref()
        .is_none_or(|depth| depth.size != token.viewport || depth.samples != token.samples)
    {
        // Drop the old declared allocation before constructing its replacement.
        gpu.depth = None;
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("bounded private hand reverse-Z depth"),
            size: Extent3d {
                width: token.viewport[0],
                height: token.viewport[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: token.samples,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        gpu.depth = Some(DepthGpu {
            _texture: texture,
            view,
            size: token.viewport,
            samples: token.samples,
        });
    }
    queue.write_buffer(
        &gpu.projection,
        0,
        bytemuck::cast_slice(&hand_projection(token.viewport).to_cols_array()),
    );
    if gpu.bind_group.is_none() {
        gpu.bind_group = Some(device.create_bind_group(
            "neutral hand binding",
            &cache.get_bind_group_layout(&gpu.layout),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: gpu.projection.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&gpu.skin.as_ref().unwrap().view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(&gpu.sampler),
                },
            ],
        ));
    }
    let gpu = &mut *gpu;
    let layout = &gpu.layout;
    gpu.pipeline =
        memoized_hand_pipeline(&mut gpu.pipeline_variants, token.samples, token.hdr, || {
            cache.queue_render_pipeline(specialized_hand_pipeline(
                layout.clone(),
                token.samples,
                token.hdr,
            ))
        });
    if gpu.pipeline.is_none() {
        ViewmodelCompletionGate::observe_stage(2, 6, Some(token));
        gate.reject(token);
        gpu.token = None;
        return;
    }
    gpu.token = Some(token);
    ViewmodelCompletionGate::observe_stage(2, 0, Some(token));
}

/// Drops device-owned state when its device observation no longer matches.
pub(super) fn invalidate_hand_resources(gpu: &mut HandGpu) {
    gpu.token = None;
    gpu.depth = None;
    gpu.bind_group = None;
    gpu.vertices = None;
    gpu.skin = None;
    gpu.geometry = None;
    gpu.pipeline = None;
    gpu.pipeline_variants = [None; 8];
    gpu.vertex_count = 0;
}

/// Releases inactive per-view state while retaining bounded pipeline variants.
pub(super) fn deactivate_hand(gpu: &mut HandGpu) {
    gpu.token = None;
    gpu.depth = None;
    // Immutable pipeline variants remain bounded and reusable on re-enable.
}

/// Keeps one pipeline per supported sample count and colour format.
pub(super) fn memoized_hand_pipeline<T: Copy>(
    entries: &mut [Option<T>; 8],
    samples: u32,
    hdr: bool,
    create: impl FnOnce() -> T,
) -> Option<T> {
    let sample = match samples {
        1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        _ => return None,
    };
    let entry = &mut entries[sample + usize::from(hdr) * 4];
    Some(*entry.get_or_insert_with(create))
}

/// Matches the hand pipeline to its camera colour and depth attachments.
pub(super) fn specialized_hand_pipeline(
    layout: BindGroupLayoutDescriptor,
    samples: u32,
    hdr: bool,
) -> RenderPipelineDescriptor {
    let mut descriptor = hand_pipeline_descriptor(layout);
    descriptor.multisample.count = samples;
    descriptor.fragment.as_mut().unwrap().targets[0]
        .as_mut()
        .unwrap()
        .format = if hdr {
        crate::SCENE_HDR_FORMAT
    } else {
        crate::SCENE_COLOR_FORMAT
    };
    descriptor
}

/// Declares the hand projection and immutable nearest-sampled skin bindings.
pub(super) fn hand_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "neutral hand layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ],
    )
}
/// Defines binary-alpha skin drawing with private reverse-Z depth.
pub(super) fn hand_pipeline_descriptor(
    layout: BindGroupLayoutDescriptor,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("neutral static empty hand".into()),
        layout: vec![layout],
        vertex: VertexState {
            shader: HAND_SHADER,
            entry_point: Some("hand_vertex".into()),
            buffers: vec![VertexBufferLayout {
                array_stride: size_of::<HandVertex>() as u64,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![
                    VertexAttribute {
                        format: VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32x2,
                        offset: 12,
                        shader_location: 1,
                    },
                ],
            }],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: HAND_SHADER,
            entry_point: Some("hand_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: crate::SCENE_COLOR_FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        depth_stencil: Some(DepthStencilState {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::GreaterEqual),
            stencil: default(),
            bias: default(),
        }),
        ..default()
    }
}

/// Publishes hand coverage once the frame's device poll sees its encoded draw complete.
pub(super) fn submit_completion(
    queue: Res<RenderQueue>,
    drawn: Res<HandDrawn>,
    gate: Res<ViewmodelCompletionGate>,
    gpu: Res<HandGpu>,
) {
    let token = drawn.0.lock().expect("hand drawn lock").take();
    if token.is_none()
        && let Some(expected) = gpu.token
        && !gate.rejected(expected)
    {
        // Coverage is per current intended view, never accumulated from earlier
        // frames. A previously completed draw cannot authorize a missing pass.
        // This render-stage rejection restores CPU at the next UI publication;
        // it cannot retroactively restore a UI frame already encoded this frame.
        gate.reject(expected);
    }
    if let Some(reservation) = token.and_then(|token| gate.reserve(token)) {
        let callback = gate.clone();
        crate::device_poll::on_frame_complete(&queue, move || {
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!("viewmodel.completion_callback").entered();
            callback.complete(reservation);
        });
        ViewmodelCompletionGate::observe_stage(4, 1, token);
    } else {
        ViewmodelCompletionGate::observe_stage(
            4,
            if token.is_some() { 2 } else { 3 },
            token.or(gpu.token),
        );
    }
}
