//! Block-entity material layouts, specialization and bounded prewarming.

use super::*;

pub(super) struct BlockEntitySpecializer;

#[derive(Resource)]
pub(super) struct BlockEntityPipeline {
    pub(super) variants: Variants<RenderPipeline, BlockEntitySpecializer>,
    pub(super) bind_group_layout: BindGroupLayoutDescriptor,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum PipelineMode {
    Solid,
    Overlay,
    Outline,
    Crack,
    Portal,
    Additive,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub(super) struct BlockEntityPipelineKey {
    pub(super) mode: PipelineMode,
    pub(super) msaa: Msaa,
    pub(super) hdr: bool,
}

impl FromWorld for BlockEntityPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "block-entity bind group layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(ViewUniform::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(VERTEX_BYTES),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(PORTAL_PARAMETER_BYTES),
                    },
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("block-entity pipeline".into()),
            layout: vec![bind_group_layout.clone(), crate::lighting::layout()],
            vertex: VertexState {
                shader: SHADER_HANDLE,
                entry_point: Some("block_entity_vertex".into()),
                buffers: vec![],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: SHADER_HANDLE,
                entry_point: Some("block_entity_solid".into()),
                targets: vec![Some(ColorTargetState {
                    format: crate::SCENE_COLOR_FORMAT,
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
            ..default()
        };
        Self {
            variants: Variants::new(BlockEntitySpecializer, descriptor),
            bind_group_layout,
        }
    }
}

impl Specializer<RenderPipeline> for BlockEntitySpecializer {
    type Key = BlockEntityPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        crate::alpha_coverage::apply(descriptor, key.mode == PipelineMode::Solid);
        descriptor.primitive.cull_mode =
            matches!(key.mode, PipelineMode::Portal | PipelineMode::Additive)
                .then_some(bevy::render::render_resource::Face::Back);
        descriptor.primitive.topology = PrimitiveTopology::TriangleList;
        descriptor.vertex.entry_point = Some(
            match key.mode {
                PipelineMode::Portal => "portal_vertex",
                PipelineMode::Outline => "selection_line_vertex",
                PipelineMode::Crack => "block_overlay_vertex",
                _ => "block_entity_vertex",
            }
            .into(),
        );
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.entry_point = Some(
            match key.mode {
                PipelineMode::Solid => "block_entity_solid",
                PipelineMode::Overlay => "block_entity_overlay",
                PipelineMode::Outline => "selection_line_fragment",
                PipelineMode::Crack => "block_entity_crack",
                PipelineMode::Portal => "portal_fragment",
                PipelineMode::Additive => "block_entity_additive",
            }
            .into(),
        );
        let target = fragment.targets[0].as_mut().unwrap();
        target.format = if key.hdr {
            crate::SCENE_HDR_FORMAT
        } else {
            crate::SCENE_COLOR_FORMAT
        };
        target.blend = match key.mode {
            PipelineMode::Solid | PipelineMode::Outline => None,
            PipelineMode::Portal => Some(BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
            }),
            PipelineMode::Overlay => Some(BlendState::ALPHA_BLENDING),
            PipelineMode::Additive => Some(super::super::dragon_death::DRAGON_DEATH_BLEND),
            // Twice source times destination, like the classic destroy overlay.
            PipelineMode::Crack => Some(BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::Dst,
                    dst_factor: BlendFactor::Src,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::Zero,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
            }),
        };
        descriptor
            .depth_stencil
            .as_mut()
            .unwrap()
            .depth_write_enabled = Some(matches!(
            key.mode,
            PipelineMode::Solid | PipelineMode::Outline | PipelineMode::Portal
        ));
        Ok(key)
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for BlockEntityPipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        use PipelineMode::*;
        for mode in [Solid, Overlay, Outline, Crack, Portal, Additive] {
            ids.push(self.variants.specialize(
                cache,
                BlockEntityPipelineKey {
                    mode,
                    msaa: view.msaa,
                    hdr: view.hdr,
                },
            )?);
        }
        Ok(())
    }
}
