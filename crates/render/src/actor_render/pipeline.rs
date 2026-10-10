use super::*;

pub(super) struct ActorPipelineSpecializer;

#[derive(Resource)]
pub(crate) struct ActorPipeline {
    pub(super) variants: Variants<RenderPipeline, ActorPipelineSpecializer>,
    pub(super) bind_group_layout: BindGroupLayoutDescriptor,
    draw_variants: std::collections::HashMap<
        ActorPipelineContract,
        bevy::render::render_resource::CachedRenderPipelineId,
    >,
}

impl FromWorld for ActorPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = actor_bind_group_layout();
        let descriptor = actor_pipeline_descriptor(bind_group_layout.clone());
        Self {
            variants: Variants::new(ActorPipelineSpecializer, descriptor),
            bind_group_layout,
            draw_variants: Default::default(),
        }
    }
}

impl ActorPipeline {
    pub(super) fn prewarm(
        &mut self,
        cache: &PipelineCache,
        msaa: Msaa,
        hdr: bool,
        enhanced: bool,
    ) -> Option<bevy::render::render_resource::CachedRenderPipelineId> {
        for material in prewarm_materials() {
            let key = ActorPipelineKey {
                msaa,
                hdr,
                enhanced,
                material,
            };
            let contract = key.contract();
            if self.draw_variants.contains_key(&contract) {
                continue;
            }
            let id = self.variants.specialize(cache, key).ok()?;
            self.draw_variants.insert(contract, id);
        }
        self.draw_variant(
            msaa,
            hdr,
            enhanced,
            assets::EntityRenderMaterial::Default as u32,
        )
    }

    pub(super) fn draw_variant(
        &self,
        msaa: Msaa,
        hdr: bool,
        enhanced: bool,
        material: u32,
    ) -> Option<bevy::render::render_resource::CachedRenderPipelineId> {
        self.draw_variants
            .get(
                &ActorPipelineKey {
                    msaa,
                    hdr,
                    enhanced,
                    material,
                }
                .contract(),
            )
            .copied()
    }

    pub(super) fn ready(
        &self,
        cache: &PipelineCache,
        msaa: Msaa,
        hdr: bool,
        enhanced: bool,
    ) -> bool {
        prewarm_materials().all(|material| {
            self.draw_variant(msaa, hdr, enhanced, material)
                .is_some_and(|id| cache.get_render_pipeline(id).is_some())
        })
    }
}

/// Requests every distinct raster contract before the first actor reaches a view.
fn prewarm_materials() -> impl Iterator<Item = u32> {
    let kinds = [
        assets::EntityRenderMaterial::Default,
        assets::EntityRenderMaterial::DissolveDepth,
        assets::EntityRenderMaterial::DissolveColor,
    ];
    let ordinary = kinds.into_iter().flat_map(|kind| {
        [false, true].into_iter().flat_map(move |cull| {
            [false, true].into_iter().flat_map(move |blend| {
                [false, true].into_iter().flat_map(move |depth_write| {
                    [false, true].into_iter().map(move |alpha_test| {
                        kind.word(Some(assets::EntityRenderMaterialState {
                            alpha_test,
                            cull,
                            blend,
                            depth_write,
                            ..Default::default()
                        }))
                    })
                })
            })
        })
    });
    let additive = kinds.into_iter().flat_map(|kind| {
        [false, true].into_iter().flat_map(move |cull| {
            [false, true].into_iter().flat_map(move |depth_write| {
                [false, true].into_iter().map(move |additive_alpha| {
                    kind.word(Some(assets::EntityRenderMaterialState {
                        cull,
                        depth_write,
                        blend: true,
                        additive: true,
                        additive_alpha,
                        ..Default::default()
                    }))
                })
            })
        })
    });
    // Pack materials such as `entity_depth` and `entity_alpha_depth` that draw through walls.
    let depth_always = [false, true].into_iter().flat_map(|cull| {
        [false, true].into_iter().map(move |alpha_test| {
            assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
                alpha_test,
                cull,
                depth_always: true,
                ..Default::default()
            }))
        })
    });
    ordinary.chain(additive).chain(depth_always)
}

pub(super) fn prepare_actor_pipelines(
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<ActorPipeline>,
    readiness: Res<crate::ActorPipelineReadiness>,
    views: Query<(&ExtractedView, &Msaa, Option<&crate::EnhancedRendering>)>,
) {
    let mut has_view = false;
    let mut ready = true;
    for (view, msaa, enhanced) in &views {
        has_view = true;
        ready &= pipeline
            .prewarm(&cache, *msaa, view.hdr, enhanced.is_some())
            .is_some()
            && pipeline.ready(&cache, *msaa, view.hdr, enhanced.is_some());
    }
    readiness.publish(has_view && ready);
}

pub(super) fn actor_bind_group_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "instanced actor bind group layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                // The fragment stage reads the camera position for distance fog.
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: Some(ViewUniform::min_size()),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorGpuInstance>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorRigVertex>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorRigGeometrySpan>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<[[f32; 4]; 3]>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 5,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<[[f32; 4]; 3]>() as u64),
                },
                count: None,
            },
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
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(16),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 12,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 13,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            // Player skin arrays of the 64, 128 and 256 texel classes.
            BindGroupLayoutEntry {
                binding: 9,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
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

pub(super) fn actor_pipeline_descriptor(
    bind_group_layout: BindGroupLayoutDescriptor,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("bounded shared actor pipeline".into()),
        layout: vec![bind_group_layout, crate::lighting::layout()],
        vertex: VertexState {
            shader: ACTOR_SHADER_HANDLE,
            entry_point: Some("actor_vertex".into()),
            buffers: vec![],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: ACTOR_SHADER_HANDLE,
            entry_point: Some("actor_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::bevy_default(),
                blend: None,
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ActorPipelineKey {
    pub(super) msaa: Msaa,
    pub(super) hdr: bool,
    pub(super) enhanced: bool,
    pub(super) material: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ActorPipelineContract {
    msaa: Msaa,
    format: TextureFormat,
    kind: assets::EntityRenderMaterial,
    cull: bool,
    blend: bool,
    depth_write: bool,
    depth_always: bool,
    additive: bool,
    additive_alpha: bool,
    alpha_to_coverage: bool,
}

impl ActorPipelineKey {
    /// Coalesces shader-only material flags while retaining distinct coverage and depth states.
    fn contract(self) -> ActorPipelineContract {
        let state = crate::actor::material::state(self.material).unwrap_or(
            assets::EntityRenderMaterialState {
                alpha_test: false,
                cull: false,
                blend: false,
                depth_write: true,
                ..Default::default()
            },
        );
        let kind = match self.material & assets::EntityRenderMaterialState::KIND_MASK {
            value if value == assets::EntityRenderMaterial::DissolveDepth as u32 => {
                assets::EntityRenderMaterial::DissolveDepth
            }
            value if value == assets::EntityRenderMaterial::DissolveColor as u32 => {
                assets::EntityRenderMaterial::DissolveColor
            }
            _ => assets::EntityRenderMaterial::Default,
        };
        let format = if self.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else if super::phase::sorted(self.material)
            && crate::chunk::transparent::gamma_pass::admitted(self.hdr, self.msaa, self.enhanced)
        {
            TextureFormat::bevy_default().remove_srgb_suffix()
        } else {
            TextureFormat::bevy_default()
        };
        ActorPipelineContract {
            msaa: self.msaa,
            format,
            kind,
            cull: state.cull,
            blend: state.blend,
            depth_write: state.depth_write,
            depth_always: state.depth_always,
            additive: state.blend && state.additive,
            additive_alpha: state.blend && state.additive && state.additive_alpha,
            alpha_to_coverage: self.msaa.samples() > 1
                && state.alpha_test
                && !state.blend
                && !state.emissive
                && self.material & assets::EntityRenderMaterialState::KIND_MASK
                    != assets::EntityRenderMaterial::Dragon as u32
                && kind == assets::EntityRenderMaterial::Default,
        }
    }
}

impl SpecializerKey for ActorPipelineKey {
    const IS_CANONICAL: bool = false;
    type Canonical = ActorPipelineContract;
}

impl Specializer<RenderPipeline> for ActorPipelineSpecializer {
    type Key = ActorPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let contract = key.contract();
        descriptor.multisample.count = contract.msaa.samples();
        crate::alpha_coverage::apply(descriptor, contract.alpha_to_coverage);
        if let Some(state) = crate::actor::material::state(key.material) {
            descriptor.primitive.cull_mode = state
                .cull
                .then_some(bevy::render::render_resource::Face::Back);
            descriptor
                .depth_stencil
                .as_mut()
                .unwrap()
                .depth_write_enabled = state.depth_write;
            if state.depth_always {
                descriptor.depth_stencil.as_mut().unwrap().depth_compare = CompareFunction::Always;
            }
            descriptor.fragment.as_mut().unwrap().targets[0]
                .as_mut()
                .unwrap()
                .blend = crate::actor::material::blend_state(state);
        }
        let kind = key.material & assets::EntityRenderMaterialState::KIND_MASK;
        if kind == assets::EntityRenderMaterial::DissolveDepth as u32 {
            descriptor.fragment.as_mut().unwrap().targets[0]
                .as_mut()
                .unwrap()
                .write_mask = ColorWrites::empty();
        } else if kind == assets::EntityRenderMaterial::DissolveColor as u32 {
            descriptor.depth_stencil.as_mut().unwrap().depth_compare = CompareFunction::Equal;
        }
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.targets[0].as_mut().unwrap().format = contract.format;
        // Sorted-pass spans share the transparent pass's gamma-encoded target.
        if super::phase::sorted(key.material)
            && crate::chunk::transparent::gamma_pass::admitted(key.hdr, key.msaa, key.enhanced)
        {
            fragment.shader_defs.push(bevy::shader::ShaderDefVal::Bool(
                "ACTOR_GAMMA_BLEND".into(),
                true,
            ));
        }
        Ok(contract)
    }
}
