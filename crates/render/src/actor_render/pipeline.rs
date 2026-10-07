//! Shared actor layouts and per-view material pipeline variants.

use super::*;

pub(super) struct ActorPipelineSpecializer;

#[derive(Resource)]
pub(super) struct ActorPipeline {
    pub(super) variants: Variants<RenderPipeline, ActorPipelineSpecializer>,
    pub(super) bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for ActorPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = actor_bind_group_layout();
        let descriptor = actor_pipeline_descriptor(bind_group_layout.clone());
        Self {
            variants: Variants::new(ActorPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

pub(crate) fn actor_bind_group_layout() -> BindGroupLayoutDescriptor {
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
                visibility: ShaderStages::VERTEX,
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
        ],
    )
}

pub(crate) fn actor_pipeline_descriptor(
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

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub(super) struct ActorPipelineKey {
    pub(super) msaa: Msaa,
    pub(super) hdr: bool,
    pub(super) enhanced: bool,
}

impl Specializer<RenderPipeline> for ActorPipelineSpecializer {
    type Key = ActorPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
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

#[cfg(feature = "enhanced")]
pub(crate) fn actor_shadow_pipeline_descriptor(
    caster_layout: BindGroupLayoutDescriptor,
    depth_format: TextureFormat,
) -> RenderPipelineDescriptor {
    use bevy::render::render_resource::PrimitiveState;

    let mut descriptor = actor_pipeline_descriptor(actor_bind_group_layout());
    descriptor.label = Some("enhanced animated actor shadow caster".into());
    descriptor.layout.push(caster_layout);
    descriptor.vertex.shader_defs = vec!["ENHANCED_SHADOW".into()];
    descriptor.fragment = Some(FragmentState {
        shader: ACTOR_SHADER_HANDLE,
        shader_defs: vec!["ENHANCED_SHADOW".into()],
        entry_point: Some("actor_fragment_shadow".into()),
        targets: vec![],
    });
    // Actor planes carry independent front/back UVs and one-sided coverage sentinels.
    descriptor.primitive = PrimitiveState {
        cull_mode: None,
        ..default()
    };
    descriptor.depth_stencil = Some(DepthStencilState {
        format: depth_format,
        depth_write_enabled: true,
        depth_compare: CompareFunction::LessEqual,
        stencil: default(),
        bias: crate::enhanced::shadow_raster_bias(),
    });
    descriptor
}

#[cfg(feature = "enhanced")]
pub(crate) fn actor_motion_pipeline_descriptor(
    caster_layout: BindGroupLayoutDescriptor,
    depth_format: TextureFormat,
) -> RenderPipelineDescriptor {
    let mut descriptor = actor_shadow_pipeline_descriptor(caster_layout, depth_format);
    descriptor.layout.push(super::motion::actor_motion_layout());
    descriptor.vertex.shader_defs.push("ENHANCED_MOTION".into());
    let fragment = descriptor.fragment.as_mut().unwrap();
    fragment.shader_defs.push("ENHANCED_MOTION".into());
    fragment.entry_point = Some("actor_fragment_motion".into());
    fragment.targets = vec![Some(ColorTargetState {
        format: TextureFormat::Rgba16Float,
        blend: None,
        write_mask: ColorWrites::ALL,
    })];
    descriptor
}
