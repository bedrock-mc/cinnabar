use super::*;

#[derive(Resource)]
pub(crate) struct EntityShadowGpu {
    pub(super) mesh: Buffer,
    pub(super) instances: Buffer,
    pub(super) capacity: u64,
    pub(super) params: Buffer,
    pub(super) params_value: Option<EntityShadowParams>,
    pub(super) uploaded_revision: Option<u64>,
    pub(super) count: u32,
    pub(super) layouts: [BindGroupLayoutDescriptor; 2],
    /// Pipelines match both the scene format and its coverage samples.
    pub(super) pipelines: Vec<((TextureFormat, u32), CachedRenderPipelineId)>,
    /// Instance and parameter uploads, for the unchanged-frame contract.
    pub(crate) uploads: u64,
}

impl EntityShadowGpu {
    /// Allocates persistent caster geometry, parameters, and depth-layout variants.
    pub(super) fn new(device: &RenderDevice) -> Self {
        let mesh = shadow_volume_mesh();
        let mesh =
            device.create_buffer_with_data(&bevy::render::render_resource::BufferInitDescriptor {
                label: Some("entity shadow volume"),
                contents: bytemuck::cast_slice(&mesh),
                usage: BufferUsages::VERTEX,
            });
        let fragment = ShaderStages::FRAGMENT;
        let entry = |binding, visibility, ty| BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let layouts = [false, true].map(|multisampled| {
            BindGroupLayoutDescriptor::new(
                "entity shadow bind group layout",
                &[
                    entry(
                        0,
                        ShaderStages::VERTEX_FRAGMENT,
                        BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            has_dynamic_offset: true,
                            min_binding_size: Some(ViewUniform::min_size()),
                        },
                    ),
                    entry(
                        1,
                        fragment,
                        BindingType::Texture {
                            sample_type: TextureSampleType::Depth,
                            view_dimension: TextureViewDimension::D2,
                            multisampled,
                        },
                    ),
                    entry(
                        3,
                        ShaderStages::VERTEX_FRAGMENT,
                        BindingType::Buffer {
                            ty: BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(INSTANCE_BYTES),
                        },
                    ),
                    entry(
                        4,
                        ShaderStages::VERTEX_FRAGMENT,
                        BindingType::Buffer {
                            ty: BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: NonZeroU64::new(
                                size_of::<EntityShadowParams>() as u64
                            ),
                        },
                    ),
                ],
            )
        });
        Self {
            mesh,
            instances: instance_buffer(device, 64),
            capacity: 64,
            params: device.create_buffer(&BufferDescriptor {
                label: Some("entity shadow parameters"),
                size: size_of::<EntityShadowParams>() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            params_value: None,
            uploaded_revision: None,
            count: 0,
            layouts,
            pipelines: Vec::new(),
            uploads: 0,
        }
    }

    /// Reuses pipelines until the attachment format or sample count changes.
    pub(super) fn pipeline(
        &mut self,
        cache: &PipelineCache,
        format: TextureFormat,
        samples: u32,
    ) -> CachedRenderPipelineId {
        let key = (format, samples);
        if let Some((_, id)) = self.pipelines.iter().find(|(known, _)| *known == key) {
            return *id;
        }
        let layout = self.layouts[usize::from(samples > 1)].clone();
        let id = cache.queue_render_pipeline(pipeline_descriptor(layout, format, samples));
        self.pipelines.push((key, id));
        id
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for EntityShadowGpu {
    /// Uses the view's format and sample count, as `prepare_shadow_views` does.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), bevy::prelude::BevyError> {
        let format = if view.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        ids.push(self.pipeline(cache, format.remove_srgb_suffix(), view.msaa.samples()));
        Ok(())
    }
}

/// Allocates storage only when the caster capacity grows.
fn instance_buffer(device: &RenderDevice, capacity: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("entity shadow casters"),
        size: capacity * INSTANCE_BYTES,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Multiplies each covered sample once, even when several shadow volumes overlap.
pub(super) fn pipeline_descriptor(
    layout: BindGroupLayoutDescriptor,
    format: TextureFormat,
    samples: u32,
) -> RenderPipelineDescriptor {
    let shader_defs = if samples > 1 {
        vec!["MULTISAMPLED".into()]
    } else {
        Vec::new()
    };
    RenderPipelineDescriptor {
        label: Some("entity shadow pipeline".into()),
        layout: vec![layout],
        vertex: VertexState {
            shader: SHADER,
            shader_defs: shader_defs.clone(),
            entry_point: Some("shadow_vertex".into()),
            buffers: vec![VertexBufferLayout {
                array_stride: 12,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![VertexAttribute {
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                }],
            }],
        },
        fragment: Some(FragmentState {
            shader: SHADER,
            shader_defs,
            entry_point: Some("shadow_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format,
                blend: Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::Zero,
                        dst_factor: BlendFactor::Src,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::Zero,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                }),
                write_mask: ColorWrites::COLOR,
            })],
        }),
        multisample: MultisampleState {
            count: samples,
            ..default()
        },
        depth_stencil: Some(DepthStencilState {
            format: TextureFormat::Stencil8,
            depth_write_enabled: false,
            depth_compare: CompareFunction::Always,
            stencil: StencilState {
                front: shadow_stencil(),
                back: shadow_stencil(),
                read_mask: 0xff,
                write_mask: 0xff,
            },
            bias: default(),
        }),
        primitive: PrimitiveState {
            cull_mode: Some(Face::Front),
            ..default()
        },
        ..default()
    }
}

/// The first accepted volume marks its sample so later volumes cannot darken it again.
fn shadow_stencil() -> StencilFaceState {
    StencilFaceState {
        compare: CompareFunction::NotEqual,
        fail_op: StencilOperation::Keep,
        depth_fail_op: StencilOperation::Keep,
        pass_op: StencilOperation::Replace,
    }
}

/// The colour multiplier for this frame's sky; Nether and End have no sunrise glow.
pub(super) fn shadow_params(atmosphere: &AtmosphereFrame) -> EntityShadowParams {
    let sunrise = if atmosphere.sky_kind() == SkyKind::Overworld {
        crate::celestial::raw_sunrise_band(atmosphere.celestial_angle())
    } else {
        [0.0; 4]
    };
    let sky = atmosphere.sky_zenith().map(linear_to_srgb);
    EntityShadowParams::new(entity_shadow_colour(sky, sunrise))
}

/// Converts the sky tint to the encoded space used by ordinary shadow blending.
pub(super) fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

/// Uploads changed casters and lighting without rebuilding unchanged resources.
pub(crate) fn prepare_shadow_buffers(
    scene: Res<EntityShadowScene>,
    atmosphere: Option<Res<AtmosphereFrame>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<EntityShadowGpu>,
) {
    let frame = &scene.0;
    if gpu.uploaded_revision != Some(frame.revision) {
        let needed = frame.shadows.len() as u64;
        if needed > gpu.capacity {
            gpu.capacity = needed.next_power_of_two();
            gpu.instances = instance_buffer(&device, gpu.capacity);
        }
        if needed > 0 {
            queue.write_buffer(&gpu.instances, 0, bytemuck::cast_slice(&frame.shadows));
            gpu.uploads += 1;
        }
        gpu.count = needed as u32;
        gpu.uploaded_revision = Some(frame.revision);
    }
    let params = shadow_params(&atmosphere.map(|frame| *frame).unwrap_or_default());
    if gpu.params_value != Some(params) {
        queue.write_buffer(&gpu.params, 0, bytemuck::bytes_of(&params));
        gpu.params_value = Some(params);
        gpu.uploads += 1;
    }
}
