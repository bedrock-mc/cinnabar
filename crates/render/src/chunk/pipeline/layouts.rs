use crate::chunk::*;

mod terrain_blend;

// Packed liquid corners run opposite to cube/model corners. Native's outward
// winding is preserved without reversing the index buffer shared with cubes.
const LIQUID_FRONT_FACE: bevy::render::render_resource::FrontFace =
    bevy::render::render_resource::FrontFace::Cw;

/// Minimum vertex storage slots required by the shared world layout.
pub fn required_vertex_storage_buffers() -> u32 {
    chunk_bind_group_layout()
        .entries
        .iter()
        .filter(|entry| {
            entry.visibility.contains(ShaderStages::VERTEX)
                && matches!(
                    entry.ty,
                    BindingType::Buffer {
                        ty: BufferBindingType::Storage { .. },
                        ..
                    }
                )
        })
        .count() as u32
}

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
        let bind_group_layout = chunk_bind_group_layout();
        let descriptor = RenderPipelineDescriptor {
            label: Some("packed chunk pipeline".into()),
            layout: vec![bind_group_layout.clone(), crate::lighting::layout()],
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
                // Native cutout leaves disable culling; opaque/deep faces keep
                // their single-sided policy through the material fragment gate.
                cull_mode: None,
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
        terrain_blend::apply(&mut transparent_model_descriptor);
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
        terrain_blend::apply(&mut liquid_descriptor);
        liquid_descriptor.primitive.cull_mode = None;
        liquid_descriptor.primitive.front_face = LIQUID_FRONT_FACE;
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
        depth_liquid_descriptor.primitive.front_face = LIQUID_FRONT_FACE;
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
    pub(in crate::chunk) enhanced: bool,
}

impl Specializer<RenderPipeline> for ChunkPipelineSpecializer {
    type Key = ChunkPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        let native_gamma = !key.hdr
            && key.msaa == Msaa::Off
            && !(render_model::ENHANCED_RENDERING_ENABLED && key.enhanced)
            && descriptor
                .fragment
                .as_ref()
                .unwrap()
                .shader_defs
                .contains(&"NATIVE_GAMMA_BLEND".into());
        if !native_gamma {
            descriptor
                .fragment
                .as_mut()
                .unwrap()
                .shader_defs
                .retain(|definition| definition != &"NATIVE_GAMMA_BLEND".into());
        }
        descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .format = if key.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else if native_gamma {
            TextureFormat::bevy_default().remove_srgb_suffix()
        } else {
            TextureFormat::bevy_default()
        };
        #[cfg(feature = "enhanced")]
        if render_model::ENHANCED_RENDERING_ENABLED && key.enhanced {
            descriptor
                .layout
                .push(crate::enhanced::enhanced_view_layout());
            descriptor.vertex.shader_defs.push("ENHANCED".into());
            descriptor
                .fragment
                .as_mut()
                .unwrap()
                .shader_defs
                .push("ENHANCED".into());
        }
        Ok(key)
    }
}

/// Shared vertex-pulling bindings used by world rendering and shadow casters.
pub(crate) fn chunk_bind_group_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
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
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: Some(AtmosphereFrame::min_size()),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::PBR_NORMAL_TEXTURE_BINDINGS[0],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::PBR_NORMAL_TEXTURE_BINDINGS[1],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::PBR_MER_TEXTURE_BINDINGS[0],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::PBR_MER_TEXTURE_BINDINGS[1],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::PBR_SAMPLER_BINDING,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::ENHANCED_COLOR_TEXTURE_BINDINGS[0],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::ENHANCED_COLOR_TEXTURE_BINDINGS[1],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::ENHANCED_NORMAL_TEXTURE_BINDINGS[0],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::ENHANCED_NORMAL_TEXTURE_BINDINGS[1],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::ENHANCED_MER_TEXTURE_BINDINGS[0],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::ENHANCED_MER_TEXTURE_BINDINGS[1],
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::ENHANCED_SAMPLER_BINDING,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::ENHANCED_TEXTURE_REF_BINDING,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    )
}

#[cfg(test)]
mod enhanced_tests {
    use super::*;

    /// Recreates the vanilla specialization before the Enhanced extension.
    fn vanilla_descriptor(msaa: Msaa, hdr: bool) -> RenderPipelineDescriptor {
        let mut descriptor = RenderPipelineDescriptor {
            fragment: Some(FragmentState {
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        };
        descriptor.multisample.count = msaa.samples();
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

    #[test]
    fn disabled_enhanced_keeps_vanilla_descriptor_and_shader_defs_identical() {
        for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
            for hdr in [false, true] {
                let mut before = vanilla_descriptor(Msaa::Off, false);
                ChunkPipelineSpecializer
                    .specialize(
                        ChunkPipelineKey {
                            msaa,
                            hdr,
                            enhanced: false,
                        },
                        &mut before,
                    )
                    .unwrap();
                let mut after = vanilla_descriptor(Msaa::Off, false);
                ChunkPipelineSpecializer
                    .specialize(
                        ChunkPipelineKey {
                            msaa,
                            hdr,
                            enhanced: true,
                        },
                        &mut after,
                    )
                    .unwrap();
                assert_eq!(format!("{before:?}"), format!("{after:?}"));
            }
        }
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_render_storage_budget_covers_the_actual_layout() {
        let count = chunk_bind_group_layout()
            .entries
            .iter()
            .filter(|entry| {
                entry.visibility.contains(ShaderStages::VERTEX)
                    && matches!(
                        entry.ty,
                        BindingType::Buffer {
                            ty: BufferBindingType::Storage { .. },
                            ..
                        }
                    )
            })
            .count() as u32;
        assert!(
            count <= required_vertex_storage_buffers(),
            "layout needs {count} vertex storage slots"
        );
    }
}

#[cfg(test)]
#[path = "contract_tests.rs"]
mod contract_tests;
