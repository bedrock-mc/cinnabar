//! Retained GPU resources, uploads and pipeline variants for the camera-local hand.

use super::*;

pub(super) struct HandRigDepth {
    pub(super) _texture: Texture,
    pub(super) view: TextureView,
    pub(super) size: [u32; 2],
    pub(super) samples: u32,
}

pub(super) struct HandRigAtlas {
    pub(super) _texture: Texture,
    pub(super) view: TextureView,
    pub(super) pixels: Arc<[u8]>,
    pub(super) size: [u32; 3],
}

pub(super) struct HandRigSkin {
    pub(super) _texture: Texture,
    pub(super) view: TextureView,
    pub(super) pixels: SkinRgba8,
}

#[derive(Resource)]
pub(super) struct HandRigGpu {
    pub(super) layout: BindGroupLayoutDescriptor,
    pub(super) sampler: Sampler,
    pub(super) view_uniform: Buffer,
    pub(super) material: Buffer,
    pub(super) light_uniform: Buffer,
    pub(super) uniforms: Option<([f32; 16], HandRigLight)>,
    #[cfg(test)]
    pub(super) uniform_uploads: [u64; 2],
    pub(super) instances: Option<Buffer>,
    pub(super) vertices: crate::actor::gpu::SegmentedVertexBuffer,
    pub(super) spans: Option<Buffer>,
    pub(super) previous_bones: Option<Buffer>,
    pub(super) current_bones: Option<Buffer>,
    pub(super) skin: Option<HandRigSkin>,
    pub(super) atlases: [Option<HandRigAtlas>; 2],
    pub(super) instance_count: u32,
    pub(super) depth: Option<HandRigDepth>,
    pub(super) bind_group: Option<BindGroup>,
    pub(super) pipeline: Option<CachedRenderPipelineId>,
    pub(super) pipeline_variants: [Option<CachedRenderPipelineId>; 8],
    pub(super) geometry_revision: Option<u64>,
    pub(super) revision: Option<u64>,
    pub(super) maximum_vertex_count: u32,
}

/// Allocates persistent rig buffers, layouts and nearest-sampled artwork bindings.
pub(super) fn init_gpu(mut commands: Commands, device: Res<RenderDevice>) {
    let uniform = |label: &'static str, contents: &[u8]| {
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some(label),
            contents,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        })
    };
    let material = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("first-person rig texture and alpha selectors"),
        contents: bytemuck::bytes_of(&HAND_MATERIAL),
        usage: BufferUsages::UNIFORM,
    });
    commands.insert_resource(HandRigGpu {
        layout: hand_rig_layout(),
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("first-person rig binary-alpha nearest"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            ..default()
        }),
        view_uniform: uniform("first-person rig view", &[0u8; 64]),
        material,
        light_uniform: uniform("first-person rig light", &[0u8; size_of::<HandRigLight>()]),
        uniforms: None,
        #[cfg(test)]
        uniform_uploads: [0; 2],
        instances: None,
        vertices: default(),
        spans: None,
        previous_bones: None,
        current_bones: None,
        skin: None,
        atlases: [None, None],
        instance_count: 0,
        depth: None,
        bind_group: None,
        pipeline: None,
        pipeline_variants: [None; 8],
        geometry_revision: None,
        revision: None,
        maximum_vertex_count: 0,
    });
}

/// Retains the current rig resources and updates only changed frame inputs.
pub(super) fn prepare(
    scene: Res<HandRigScene>,
    (background, staging): (
        Option<Res<crate::panorama::PanoramaScene>>,
        Option<Res<crate::upload_staging::BufferUploadStaging>>,
    ),
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    mut gpu: ResMut<HandRigGpu>,
    views: Query<(&ExtractedView, &Msaa)>,
) {
    if background.is_some_and(|background| !background.game_visible()) {
        deactivate(&mut gpu);
        return;
    }
    let Some(frame) = &scene.frame else {
        deactivate(&mut gpu);
        return;
    };
    // The near-camera pass targets the widest 3d view (the main camera).
    let Some((viewport, samples, hdr)) = views
        .iter()
        .map(|(view, msaa)| (view.viewport, msaa.samples(), view.hdr))
        .filter(|(viewport, _, _)| viewport.z != 0 && viewport.w != 0)
        .max_by_key(|(viewport, _, _)| u64::from(viewport.z) * u64::from(viewport.w))
    else {
        deactivate(&mut gpu);
        return;
    };
    let size = [viewport.z, viewport.w];
    upload_geometry(&mut gpu, &device, &queue, frame);
    upload_pose(&mut gpu, &device, &queue, staging.as_deref(), frame);
    upload_skin(&mut gpu, &device, &queue, frame);
    upload_atlas(&mut gpu, &device, &queue, frame);
    ensure_depth(&mut gpu, &device, size, samples);
    let aspect = viewport.z as f32 / viewport.w as f32;
    let projection =
        Mat4::perspective_infinite_reverse_rh(frame.fov_radians, aspect, CAMERA_NEAR_PLANE_BLOCKS);
    upload_uniforms(
        &mut gpu,
        &device,
        &queue,
        staging.as_deref(),
        projection,
        frame.light,
    );
    build_bind_group(&mut gpu, &device, &cache);
    let gpu = &mut *gpu;
    let layout = gpu.layout.clone();
    gpu.pipeline = memoized_pipeline(&mut gpu.pipeline_variants, samples, hdr, || {
        cache.queue_render_pipeline(specialized_pipeline(layout.clone(), samples, hdr))
    });
    if gpu.bind_group.is_none() || gpu.pipeline.is_none() {
        gpu.maximum_vertex_count = 0;
    }
}

/// Keeps unchanged hand projection and lighting out of the staging allocation path.
pub(super) fn upload_uniforms(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    staging: Option<&crate::upload_staging::BufferUploadStaging>,
    projection: Mat4,
    light: HandRigLight,
) {
    let projection = projection.to_cols_array();
    if gpu.uniforms.as_ref().is_none_or(|old| old.0 != projection) {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "hand.projection_write",
            bytes = std::mem::size_of_val(&projection)
        )
        .entered();
        crate::upload_staging::write_batch(
            staging,
            device,
            queue,
            &[(&gpu.view_uniform, 0, bytemuck::cast_slice(&projection))],
        );
        #[cfg(test)]
        {
            gpu.uniform_uploads[0] += 1;
        }
    }
    if gpu.uniforms.as_ref().is_none_or(|old| old.1 != light) {
        #[cfg(feature = "tracy")]
        let _span =
            bevy::log::info_span!("hand.light_write", bytes = std::mem::size_of_val(&light))
                .entered();
        crate::upload_staging::write_batch(
            staging,
            device,
            queue,
            &[(&gpu.light_uniform, 0, bytemuck::bytes_of(&light))],
        );
        #[cfg(test)]
        {
            gpu.uniform_uploads[1] += 1;
        }
    }
    gpu.uniforms = Some((projection, light));
}

/// Clears draw admission while retaining reusable GPU allocations.
pub(super) fn deactivate(gpu: &mut HandRigGpu) {
    gpu.bind_group = None;
    gpu.maximum_vertex_count = 0;
    gpu.instance_count = 0;
    gpu.revision = None;
}

/// Replaces geometry storage only when the admitted geometry changes.
pub(super) fn upload_geometry(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    frame: &HandRigFrame,
) {
    if gpu.geometry_revision == Some(frame.rig.geometry_revision)
        && gpu.vertices.buffer().is_some()
        && gpu.spans.is_some()
    {
        return;
    }
    gpu.vertices.sync(
        device,
        queue,
        "first-person rig vertices",
        &frame.rig.geometry_vertices,
    );
    gpu.spans = Some(storage(
        device,
        "first-person rig spans",
        &frame.rig.geometry_spans,
    ));
    gpu.geometry_revision = Some(frame.rig.geometry_revision);
    gpu.bind_group = None;
}

/// Uploads the current rig pose and instance stream into retained buffers.
pub(super) fn upload_pose(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    staging: Option<&crate::upload_staging::BufferUploadStaging>,
    frame: &HandRigFrame,
) {
    if gpu.revision == Some(frame.revision) && gpu.instances.is_some() {
        return;
    }
    // The pose changes every frame; same-sized buffers are rewritten so the bind group survives.
    let mut recreated = false;
    for (slot, label, bytes) in [
        (
            &mut gpu.instances,
            "first-person rig instance",
            bytemuck::cast_slice::<_, u8>(&frame.rig.instances),
        ),
        (
            &mut gpu.previous_bones,
            "first-person rig previous bones",
            bytemuck::cast_slice::<_, u8>(&frame.rig.previous_bones),
        ),
        (
            &mut gpu.current_bones,
            "first-person rig current bones",
            bytemuck::cast_slice::<_, u8>(&frame.rig.current_bones),
        ),
    ] {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "hand.pose_upload",
            label,
            revision = frame.revision,
            bytes = bytes.len()
        )
        .entered();
        match slot {
            Some(buffer) if buffer.size() == bytes.len() as u64 => {
                crate::upload_staging::write_batch(staging, device, queue, &[(buffer, 0, bytes)]);
            }
            _ => {
                *slot = Some(device.create_buffer_with_data(&BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                }));
                recreated = true;
            }
        }
    }
    gpu.maximum_vertex_count = frame.rig.maximum_vertex_count;
    gpu.instance_count = u32::try_from(frame.rig.instances.len()).unwrap_or(0);
    gpu.revision = Some(frame.revision);
    if recreated {
        gpu.bind_group = None;
    }
}

/// Reuses the skin texture until its artwork changes.
pub(super) fn upload_skin(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    frame: &HandRigFrame,
) {
    if gpu
        .skin
        .as_ref()
        .is_some_and(|skin| skin.pixels == frame.skin)
    {
        return;
    }
    let side = render_model::STANDARD_SKIN_SIDE as u32;
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("first-person rig skin"),
            size: Extent3d {
                width: side,
                height: side,
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
        &frame.skin,
    );
    let view = texture.create_view(&TextureViewDescriptor {
        label: Some("first-person rig skin layer"),
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    gpu.skin = Some(HandRigSkin {
        _texture: texture,
        view,
        pixels: frame.skin.clone(),
    });
    gpu.bind_group = None;
}

/// Retains each hand atlas until its dimensions or artwork changes.
pub(super) fn upload_atlas(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    frame: &HandRigFrame,
) {
    for (slot, atlas) in gpu.atlases.iter_mut().zip(&frame.item_atlases) {
        let Some(atlas) = atlas else {
            if slot.take().is_some() {
                gpu.bind_group = None;
            }
            continue;
        };
        let limits = device.limits();
        if u32::from(atlas.width) > limits.max_texture_dimension_2d
            || u32::from(atlas.height) > limits.max_texture_dimension_2d
            || atlas.layers > limits.max_texture_array_layers
        {
            if slot.take().is_some() {
                gpu.bind_group = None;
            }
            bevy::log::warn!("first-person item atlas exceeds device texture limits");
            continue;
        }
        if slot.as_ref().is_some_and(|current| {
            Arc::ptr_eq(&current.pixels, &atlas.rgba8)
                && current.size
                    == [
                        u32::from(atlas.width),
                        u32::from(atlas.height),
                        atlas.layers,
                    ]
        }) {
            continue;
        }
        let texture = device.create_texture_with_data(
            queue,
            &TextureDescriptor {
                label: Some("first-person item atlas"),
                size: Extent3d {
                    width: u32::from(atlas.width),
                    height: u32::from(atlas.height),
                    depth_or_array_layers: atlas.layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &atlas.rgba8,
        );
        let view = texture.create_view(&TextureViewDescriptor {
            label: Some("first-person item atlas layers"),
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        });
        *slot = Some(HandRigAtlas {
            _texture: texture,
            view,
            pixels: Arc::clone(&atlas.rgba8),
            size: [
                u32::from(atlas.width),
                u32::from(atlas.height),
                atlas.layers,
            ],
        });
        gpu.bind_group = None;
    }
}

/// Matches the private hand depth attachment to the scene extent and sample count.
pub(super) fn ensure_depth(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    size: [u32; 2],
    samples: u32,
) {
    if gpu
        .depth
        .as_ref()
        .is_some_and(|depth| depth.size == size && depth.samples == samples)
    {
        return;
    }
    gpu.depth = None;
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("first-person rig private reverse-Z depth"),
        size: Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: TextureDimension::D2,
        format: CORE_3D_DEPTH_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    gpu.depth = Some(HandRigDepth {
        _texture: texture,
        view,
        size,
        samples,
    });
}

/// Binds the retained rig, lighting and artwork resources for drawing.
pub(super) fn build_bind_group(gpu: &mut HandRigGpu, device: &RenderDevice, cache: &PipelineCache) {
    if gpu.bind_group.is_some() {
        return;
    }
    let (Some(instances), Some(vertices), Some(spans), Some(previous), Some(current), Some(skin)) = (
        gpu.instances.as_ref(),
        gpu.vertices.buffer(),
        gpu.spans.as_ref(),
        gpu.previous_bones.as_ref(),
        gpu.current_bones.as_ref(),
        gpu.skin.as_ref(),
    ) else {
        return;
    };
    gpu.bind_group = Some(
        device.create_bind_group(
            "first-person rig bind group",
            &cache.get_bind_group_layout(&gpu.layout),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: gpu.view_uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: instances.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: vertices.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: spans.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: previous.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: current.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: BindingResource::TextureView(&skin.view),
                },
                BindGroupEntry {
                    binding: 7,
                    resource: BindingResource::Sampler(&gpu.sampler),
                },
                BindGroupEntry {
                    binding: 8,
                    resource: gpu.material.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 9,
                    resource: gpu.light_uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 10,
                    // Without an item atlas the skin view stands in; no instance selects it.
                    resource: BindingResource::TextureView(
                        gpu.atlases[0]
                            .as_ref()
                            .map_or(&skin.view, |atlas| &atlas.view),
                    ),
                },
                BindGroupEntry {
                    binding: 11,
                    resource: BindingResource::TextureView(
                        gpu.atlases[1]
                            .as_ref()
                            .map_or(&skin.view, |atlas| &atlas.view),
                    ),
                },
            ],
        ),
    );
}

/// Creates an immutable storage buffer for the supplied packed geometry.
pub(super) fn storage<T: bytemuck::Pod>(
    device: &RenderDevice,
    label: &'static str,
    data: &[T],
) -> Buffer {
    device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(data),
        usage: BufferUsages::STORAGE,
    })
}

/// Keeps one pipeline per supported sample count and colour format.
pub(super) fn memoized_pipeline<T: Copy>(
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

/// Matches rig drawing to the camera colour format and sample count.
pub(super) fn specialized_pipeline(
    layout: BindGroupLayoutDescriptor,
    samples: u32,
    hdr: bool,
) -> RenderPipelineDescriptor {
    let mut descriptor = pipeline_descriptor(layout);
    descriptor.multisample.count = samples;
    descriptor.fragment.as_mut().unwrap().targets[0]
        .as_mut()
        .unwrap()
        .format = if hdr {
        ViewTarget::TEXTURE_FORMAT_HDR
    } else {
        TextureFormat::bevy_default()
    };
    descriptor
}

/// Defines lit rig drawing with straight-alpha blending and private reverse-Z depth.
pub(super) fn pipeline_descriptor(layout: BindGroupLayoutDescriptor) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("first-person animated rig".into()),
        layout: vec![layout, crate::lighting::layout()],
        vertex: VertexState {
            shader: HAND_RIG_SHADER,
            entry_point: Some("hand_vertex".into()),
            buffers: vec![],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: HAND_RIG_SHADER,
            entry_point: Some("hand_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::bevy_default(),
                blend: Some(BlendState::ALPHA_BLENDING),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        depth_stencil: Some(DepthStencilState {
            format: CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: CompareFunction::GreaterEqual,
            stencil: default(),
            bias: default(),
        }),
        ..default()
    }
}

/// Declares the rig, pose, lighting and hand artwork bindings.
pub(super) fn hand_rig_layout() -> BindGroupLayoutDescriptor {
    let storage_entry = |binding: u32, min: u64| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: BufferSize::new(min),
        },
        count: None,
    };
    BindGroupLayoutDescriptor::new(
        "first-person rig bind group layout",
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
            storage_entry(1, size_of::<ActorGpuInstance>() as u64),
            storage_entry(2, size_of::<ActorRigVertex>() as u64),
            storage_entry(3, size_of::<ActorRigGeometrySpan>() as u64),
            storage_entry(4, size_of::<[[f32; 4]; 3]>() as u64),
            storage_entry(5, size_of::<[[f32; 4]; 3]>() as u64),
            BindGroupLayoutEntry {
                binding: 6,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 7,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 8,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<HandMaterialUniform>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 9,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<HandRigLight>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 10,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 11,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
        ],
    )
}

impl crate::pipeline_warmup::PrewarmPipelines for HandRigGpu {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        let layout = &self.layout;
        let samples = view.msaa.samples();
        let id = memoized_pipeline(&mut self.pipeline_variants, samples, view.hdr, || {
            cache.queue_render_pipeline(specialized_pipeline(layout.clone(), samples, view.hdr))
        })
        .ok_or("unsupported hand rig pipeline sample count")?;
        ids.push(id);
        Ok(())
    }
}
