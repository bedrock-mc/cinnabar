use crate::chunk::*;
use crate::device_settings::vertex_storage_buffers;

#[cfg(test)]
mod cutout_contact_tests;
mod terrain_blend;
mod terrain_cutout;

// Packed liquid corners run opposite to cube/model corners. Native's outward
// winding is preserved without reversing the index buffer shared with cubes; the
// transparent pipeline, which also draws models, flips the facing test for water instead.
const LIQUID_FRONT_FACE: bevy::render::render_resource::FrontFace =
    bevy::render::render_resource::FrontFace::Cw;

/// Vertex storage slots used by the preferred world layout.
pub fn required_vertex_storage_buffers() -> u32 {
    vertex_storage_buffers(&chunk_bind_group_layout())
}

/// Minimum storage slots used by the compact terrain layout.
pub fn minimum_vertex_storage_buffers() -> u32 {
    vertex_storage_buffers(&compact_chunk_bind_group_layout())
}

pub(in crate::chunk) struct ChunkPipelineSpecializer;

#[derive(Resource)]
pub(crate) struct ChunkPipeline {
    pub(in crate::chunk) variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    pub(in crate::chunk) unit_variants: [Variants<RenderPipeline, ChunkPipelineSpecializer>; 4],
    pub(in crate::chunk) unit_supported: bool,
    pub(in crate::chunk) compact: bool,
    pub(in crate::chunk) solid_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    pub(in crate::chunk) model_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    /// Sorted transparent liquid and model draws, one pipeline so they never switch programs.
    pub(in crate::chunk) transparent_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    pub(in crate::chunk) depth_liquid_variants: Variants<RenderPipeline, ChunkPipelineSpecializer>,
    pub(in crate::chunk) bind_group_layout: BindGroupLayoutDescriptor,
    pub(in crate::chunk) transparent_bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for ChunkPipeline {
    fn from_world(world: &mut World) -> Self {
        let compact = world.get_resource::<RenderDevice>().is_some_and(|device| {
            device.limits().max_storage_buffers_per_shader_stage < required_vertex_storage_buffers()
        });
        let transparent_bind_group_layout = if compact {
            compact_chunk_bind_group_layout()
        } else {
            chunk_bind_group_layout()
        };
        let bind_group_layout = if compact {
            transparent_bind_group_layout.clone()
        } else {
            opaque_chunk_bind_group_layout()
        };
        let mut descriptor = RenderPipelineDescriptor {
            label: Some("packed chunk pipeline".into()),
            layout: vec![bind_group_layout.clone(), crate::lighting::layout()],
            vertex: VertexState {
                shader: CHUNK_SHADER_HANDLE,
                // Opaque terrain fetches its vertex index and first instance; see `chunk.wesl`.
                buffers: vec![draw_offsets_layout()],
                ..default()
            },
            fragment: Some(FragmentState {
                shader: CHUNK_SHADER_HANDLE,
                entry_point: Some("fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: render_scene::SCENE_COLOR_FORMAT,
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
                depth_write_enabled: Some(true),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        if compact {
            descriptor.vertex.shader_defs.push("PACKED_TERRAIN".into());
            descriptor
                .fragment
                .as_mut()
                .unwrap()
                .shader_defs
                .push("PACKED_TERRAIN".into());
        }
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
        // Single-sided opaque cube runs: hardware culling replaces both fragment discards.
        let mut solid_descriptor = descriptor.clone();
        solid_descriptor.label = Some("packed solid chunk pipeline".into());
        solid_descriptor.primitive.cull_mode = Some(bevy::render::render_resource::Face::Back);
        solid_descriptor
            .fragment
            .as_mut()
            .expect("solid fragment")
            .entry_point = Some("fragment_solid".into());
        let mut transparent_descriptor = descriptor.clone();
        transparent_descriptor.label = Some("packed transparent terrain pipeline".into());
        transparent_descriptor.vertex.shader = TRANSPARENT_SHADER_HANDLE;
        transparent_descriptor.vertex.entry_point = Some("vertex".into());
        // Sorted transparency is never GPU-culled and indexes its refs by instance.
        transparent_descriptor.vertex.buffers.clear();
        transparent_descriptor.layout[0] = transparent_bind_group_layout.clone();
        let transparent_fragment = transparent_descriptor
            .fragment
            .as_mut()
            .expect("transparent fragment");
        transparent_fragment.shader = TRANSPARENT_SHADER_HANDLE;
        transparent_fragment.entry_point = Some("fragment".into());
        terrain_blend::apply(&mut transparent_descriptor);
        transparent_descriptor.primitive.cull_mode = None;
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
        // Apply after cloning: model geometry includes the opaque stairs/slabs
        // that must win a shared plane against a two-sided cutout cube.
        terrain_cutout::apply(&mut descriptor);
        let unit_variants = [
            &solid_descriptor,
            &descriptor,
            &model_descriptor,
            &depth_liquid_descriptor,
        ]
        .map(|descriptor| Variants::new(ChunkPipelineSpecializer, unit_descriptor(descriptor)));
        Self {
            compact,
            unit_variants,
            unit_supported: false,
            variants: Variants::new(ChunkPipelineSpecializer, descriptor),
            solid_variants: Variants::new(ChunkPipelineSpecializer, solid_descriptor),
            model_variants: Variants::new(ChunkPipelineSpecializer, model_descriptor),
            transparent_variants: Variants::new(ChunkPipelineSpecializer, transparent_descriptor),
            depth_liquid_variants: Variants::new(ChunkPipelineSpecializer, depth_liquid_descriptor),
            bind_group_layout,
            transparent_bind_group_layout,
        }
    }
}

/// Reuses all raster state and reconstruction, changing only how the vertex addresses arrive.
fn unit_descriptor(source: &RenderPipelineDescriptor) -> RenderPipelineDescriptor {
    let mut descriptor = source.clone();
    descriptor.vertex.buffers.clear();
    descriptor.vertex.entry_point = Some("vertex_unit".into());
    descriptor.vertex.shader_defs.push("TERRAIN_UNITS".into());
    descriptor
        .fragment
        .as_mut()
        .expect("terrain fragment")
        .shader_defs
        .push("TERRAIN_UNITS".into());
    descriptor.layout.push(super::units::bind_group_layout());
    descriptor
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
        let cutout = descriptor.fragment.as_ref().is_some_and(|fragment| {
            fragment.entry_point.as_deref() == Some("fragment")
                && (fragment.shader == CHUNK_SHADER_HANDLE
                    || fragment.shader == MODEL_SHADER_HANDLE)
        });
        crate::alpha_coverage::apply(descriptor, cutout);
        let native_gamma =
            super::super::transparent::gamma_pass::admitted(key.hdr, key.msaa, key.enhanced)
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
            render_scene::SCENE_HDR_FORMAT
        } else if native_gamma {
            render_scene::SCENE_COLOR_FORMAT.remove_srgb_suffix()
        } else {
            render_scene::SCENE_COLOR_FORMAT
        };
        #[cfg(feature = "enhanced")]
        if render_model::enhanced_rendering_enabled() && key.enhanced {
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

/// Vertex buffer 0 of every opaque terrain pipeline: per vertex, its vertex index, then the
/// first instance the builtin instance index lacks. Vertex fetch applies the draw's base vertex
/// on every backend, unlike the builtin `vertex_index` of DX12 count draws.
pub(crate) fn draw_offsets_layout() -> bevy::mesh::VertexBufferLayout {
    use bevy::{
        mesh::VertexBufferLayout,
        render::render_resource::{VertexAttribute, VertexFormat, VertexStepMode},
    };
    VertexBufferLayout {
        array_stride: crate::chunk::gpu_cull::model::OFFSET_ENTRY_BYTES,
        step_mode: VertexStepMode::Vertex,
        attributes: vec![VertexAttribute {
            format: VertexFormat::Uint32x2,
            offset: 0,
            shader_location: 0,
        }],
    }
}

/// Omits sorted transparency refs so Metal has room for vertex offsets and buffer sizes.
pub(crate) fn opaque_chunk_bind_group_layout() -> BindGroupLayoutDescriptor {
    let mut layout = chunk_bind_group_layout();
    layout
        .entries
        .retain(|entry| entry.binding != TRANSPARENT_REFS_BINDING);
    layout
}

/// Keeps the three large streams separate and packs the remaining tables into binding two.
pub(crate) fn compact_chunk_bind_group_layout() -> BindGroupLayoutDescriptor {
    let mut layout = chunk_bind_group_layout();
    layout.entries.retain_mut(|entry| {
        if !matches!(
            entry.ty,
            BindingType::Buffer {
                ty: BufferBindingType::Storage { .. },
                ..
            }
        ) {
            return true;
        }
        if entry.binding == 2 {
            entry.visibility = ShaderStages::VERTEX_FRAGMENT;
        }
        matches!(entry.binding, 1 | 2 | 7 | 13)
    });
    layout
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
                binding: 19,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 20,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 21,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 16,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 17,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 22,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 23,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 24,
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
                binding: TRANSPARENT_REFS_BINDING,
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
                binding: crate::material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: crate::material_shader::BIOME_QUERY_TABLES_BINDING,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: std::num::NonZeroU64::new(
                        (meshing::biome_lattice::BIOME_QUERY_TABLE_WORDS * 4) as u64,
                    ),
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
                    format: render_scene::SCENE_COLOR_FORMAT,
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
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
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

    /// Capable GPUs retain the original separate storage layouts for every terrain pass.
    #[test]
    fn desktop_terrain_keeps_the_existing_storage_layout() {
        for available in [
            required_vertex_storage_buffers(),
            required_vertex_storage_buffers() + 1,
        ] {
            let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor {
                required_limits: wgpu::Limits {
                    max_storage_buffers_per_shader_stage: available,
                    ..default()
                },
                ..default()
            });
            let mut world = World::new();
            world.insert_resource(RenderDevice::from(device));
            world.init_resource::<ChunkPipeline>();
            let pipeline = world.resource::<ChunkPipeline>();
            assert!(!pipeline.compact);
            assert_eq!(pipeline.bind_group_layout, opaque_chunk_bind_group_layout());
            assert_eq!(
                pipeline.transparent_bind_group_layout,
                chunk_bind_group_layout()
            );
        }
    }

    #[test]
    fn compact_layout_matches_its_requirement_and_all_terrain_programs() {
        let layout = compact_chunk_bind_group_layout();
        assert_eq!(
            vertex_storage_buffers(&layout),
            minimum_vertex_storage_buffers()
        );
        assert!(minimum_vertex_storage_buffers() <= 4);
        for source in [
            include_str!("../../chunk.wesl"),
            include_str!("../../model.wesl"),
            include_str!("../../liquid.wesl"),
            include_str!("../../transparent_terrain.wesl"),
        ] {
            crate::shader_test_support::assert_binding_visibility(
                &crate::shader_source::composed(source, &["PACKED_TERRAIN"]),
                0,
                &layout,
            );
        }
    }
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

impl crate::pipeline_warmup::PrewarmPipelines for ChunkPipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        let key = ChunkPipelineKey {
            msaa: view.msaa,
            hdr: view.hdr,
            enhanced: view.enhanced,
        };
        for variants in [
            &mut self.variants,
            &mut self.solid_variants,
            &mut self.model_variants,
            &mut self.transparent_variants,
            &mut self.depth_liquid_variants,
        ] {
            ids.push(variants.specialize(cache, key)?);
        }
        if self.unit_supported && !view.enhanced {
            for variants in &mut self.unit_variants {
                ids.push(variants.specialize(cache, key)?);
            }
        }
        Ok(())
    }
}
