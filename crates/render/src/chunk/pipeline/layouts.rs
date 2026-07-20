use crate::chunk::*;

const MODEL_TEMPLATE_BINDING_BUDGET: u32 = 8;
const MODEL_VERTEX_STORAGE_BINDINGS: u32 = 8;
const _: () = assert!(MODEL_VERTEX_STORAGE_BINDINGS <= MODEL_TEMPLATE_BINDING_BUDGET);

pub(in crate::chunk) struct ChunkPipelineSpecializer;

#[derive(Resource)]
pub(in crate::chunk) struct ChunkPipeline {
    pub(in crate::chunk) variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    pub(in crate::chunk) model_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    pub(in crate::chunk) transparent_model_variants:
        Variants<RenderPipeline, ChunkPipelineSpecializer>,
    pub(in crate::chunk) liquid_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    pub(in crate::chunk) depth_liquid_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    pub(in crate::chunk) bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for ChunkPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "chunk vertex-pulling bind group layout",
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
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 5,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 6,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 7,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 8,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 9,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 10,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 11,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(ChunkAnimationClock::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 12,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 13,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 14,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 15,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(AtmosphereFrame::min_size()),
                    },
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("packed chunk pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: CHUNK_SHADER_HANDLE,
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: CHUNK_SHADER_HANDLE,
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            primitive: PrimitiveState {
                cull_mode: Some(CullFace::Back),
                ..default()
            },
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: CompareFunction::GreaterEqual,
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        let mut model_descriptor = descriptor.clone();
        model_descriptor.label = Some("packed model pipeline".into());
        model_descriptor.vertex.shader = MODEL_SHADER_HANDLE;
        model_descriptor
            .fragment
            .as_mut()
            .expect("model fragment")
            .shader = MODEL_SHADER_HANDLE;
        model_descriptor
            .fragment
            .as_mut()
            .expect("model fragment")
            .entry_point = Some("fragment".into());
        model_descriptor.primitive.cull_mode = None;
        let mut transparent_model_descriptor = model_descriptor.clone();
        transparent_model_descriptor.label = Some("packed transparent model pipeline".into());
        let transparent_model_fragment = transparent_model_descriptor
            .fragment
            .as_mut()
            .expect("transparent model fragment");
        transparent_model_fragment.entry_point = Some("fragment_blend".into());
        transparent_model_fragment.targets[0]
            .as_mut()
            .expect("transparent model colour target")
            .blend = Some(BlendState::ALPHA_BLENDING);
        transparent_model_descriptor
            .depth_stencil
            .as_mut()
            .expect("transparent model depth state")
            .depth_write_enabled = false;
        let mut liquid_descriptor = descriptor.clone();
        liquid_descriptor.label = Some("packed transparent liquid pipeline".into());
        liquid_descriptor.vertex.shader = LIQUID_SHADER_HANDLE;
        liquid_descriptor.vertex.entry_point = Some("vertex".into());
        liquid_descriptor
            .fragment
            .as_mut()
            .expect("liquid fragment")
            .shader = LIQUID_SHADER_HANDLE;
        liquid_descriptor
            .fragment
            .as_mut()
            .expect("liquid fragment")
            .entry_point = Some("fragment".into());
        liquid_descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .blend = Some(BlendState::ALPHA_BLENDING);
        liquid_descriptor
            .depth_stencil
            .as_mut()
            .expect("liquid depth state")
            .depth_write_enabled = false;
        liquid_descriptor
            .depth_stencil
            .as_mut()
            .expect("liquid depth state")
            .depth_compare = CompareFunction::GreaterEqual;
        liquid_descriptor.primitive.cull_mode = None;
        let mut depth_liquid_descriptor = descriptor.clone();
        depth_liquid_descriptor.label = Some("packed depth-writing liquid pipeline".into());
        depth_liquid_descriptor.vertex.shader = LIQUID_SHADER_HANDLE;
        depth_liquid_descriptor.vertex.entry_point = Some("vertex_depth".into());
        let depth_fragment = depth_liquid_descriptor
            .fragment
            .as_mut()
            .expect("depth-writing liquid fragment");
        depth_fragment.shader = LIQUID_SHADER_HANDLE;
        depth_fragment.entry_point = Some("fragment_depth".into());
        depth_liquid_descriptor.primitive.cull_mode = None;
        Self {
            variants: Variants::new(ChunkPipelineSpecializer, descriptor),
            model_variants: Variants::new(ChunkPipelineSpecializer, model_descriptor),
            transparent_model_variants: Variants::new(
                ChunkPipelineSpecializer,
                transparent_model_descriptor,
            ),
            liquid_variants: Variants::new(ChunkPipelineSpecializer, liquid_descriptor),
            depth_liquid_variants: Variants::new(ChunkPipelineSpecializer, depth_liquid_descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub(in crate::chunk) struct ChunkPipelineKey {
    pub(in crate::chunk) msaa: Msaa,
    pub(in crate::chunk) hdr: bool,
}

impl Specializer<RenderPipeline> for ChunkPipelineSpecializer {
    type Key = ChunkPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .format = if key.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        Ok(key)
    }
}
