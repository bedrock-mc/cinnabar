//! The skinning kernel (`skin.wesl`): shader registration, generated constants, layouts,
//! cached pipelines and the job records the host writes.

use std::num::NonZeroU64;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::prelude::{App, FromWorld, Handle, Resource, Shader, World, default};
use bevy::render::render_resource::{
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
    BufferBindingType, CachedComputePipelineId, ComputePipeline, ComputePipelineDescriptor,
    PipelineCache, ShaderStages,
};
use bytemuck::{Pod, Zeroable};

use crate::shaderpack::geometry::substitute_layout_constants;

const SKIN_SHADER: Handle<Shader> = uuid_handle!("8c3d6a51-0f7e-4b2a-9d14-6e5b7a2c9f01");

/// Invocations per skinning workgroup; matches `SKIN_WORKGROUP_SIZE` in the kernel.
pub(crate) const SKIN_WORKGROUP_SIZE: u32 = 64;
/// Bytes of the kernel's `SkinParams` uniform.
pub(crate) const SKIN_PARAMS_BYTES: u64 = 32;
/// The job's positions are already in view space (the first-person hand).
pub(crate) const JOB_VIEW_SPACE: u32 = 1;
/// The instance's material culls back faces even where the rig authors them.
pub(crate) const JOB_ONE_SIDED: u32 = 2;
/// Writes glint colour and sprite-local texture coordinates instead of base artwork values.
pub(crate) const JOB_GLINT: u32 = 4;

/// One instance to skin, as the kernel's `SkinJob`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Pod, Zeroable)]
pub(crate) struct SkinJob {
    pub instance: u32,
    pub first_output: u32,
    pub vertex_count: u32,
    pub flags: u32,
    /// Block light in bits 0..4, sky light in bits 4..8.
    pub light: u32,
    /// Gamma RGBA8 vertex colour.
    pub colour: u32,
    pub block_id: i32,
    pub first_output_high: u32,
}

impl SkinJob {
    /// The full output address used when selecting a page for this job.
    pub(crate) fn output_start(&self) -> u64 {
        u64::from(self.first_output) | (u64::from(self.first_output_high) << 32)
    }

    /// Output vertices the job writes: its front faces, then as many back faces.
    pub(crate) fn output_vertices(&self) -> u32 {
        self.vertex_count * 2
    }
}

/// The kernel's `SkinParams`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub(crate) struct SkinParams {
    pub camera: [f32; 4],
    /// Rig dispatches: jobs, first job. Block-entity dispatches: vertices, first output
    /// vertex, `mc_Entity` bits.
    pub counts: [u32; 4],
}

const _: () = assert!(std::mem::size_of::<SkinParams>() as u64 == SKIN_PARAMS_BYTES);

/// Resolves the kernel's generated constants: the rig strides, packed-material constants,
/// the block-entity vertex stride and the Iris vertex layout.
pub(crate) fn skin_shader_source(source: &str) -> String {
    substitute_layout_constants(
        &crate::material_shader::source(source)
            .replace("JOB_GLINT_FLAG", &format!("{JOB_GLINT}u"))
            .replace(
                "ACTOR_GLINT_WORD",
                &format!(
                    "{}u",
                    std::mem::offset_of!(render_model::ActorGpuInstance, glint) / 4
                ),
            )
            .replace(
                "ACTOR_VERTEX_WORDS",
                &format!("{}u", super::layout::ACTOR_VERTEX_WORDS),
            )
            .replace(
                "ACTOR_MID_UV_WORD",
                &format!("{}u", super::layout::ACTOR_MID_UV_WORD),
            )
            .replace(
                "ACTOR_GPU_INSTANCE_WORDS",
                &format!("{}u", render_model::ACTOR_GPU_INSTANCE_WORDS),
            )
            .replace(
                "ACTOR_RIG_VERTEX_WORDS",
                &format!("{}u", render_model::ACTOR_RIG_VERTEX_WORDS),
            )
            .replace(
                "BLOCK_ENTITY_VERTEX_WORDS",
                &format!(
                    "{}u",
                    render_model::block_entity::mesh::BLOCK_ENTITY_VERTEX_WORDS
                ),
            ),
    )
}

fn shader(source: &str, path: impl Into<String>) -> Shader {
    crate::shader_safety::from_wesl(skin_shader_source(source), path)
}

/// Registers the kernel and the rig module it imports with Bevy's shader composer.
pub(crate) fn load_shaders(app: &mut App) {
    crate::actor_render::load_rig_shader(app);
    load_internal_asset!(app, SKIN_SHADER, "skin.wesl", shader);
}

fn storage(binding: u32, read_only: bool) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Group 0 of the rig kernel: the actor rig's instance, vertex, span and bone buffers.
pub(crate) fn rig_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "shader-pack actor skinning sources",
        &(0..5)
            .map(|binding| storage(binding, true))
            .collect::<Vec<_>>(),
    )
}

/// Group 0 of the block-entity kernel: one block-entity vertex list.
pub(crate) fn block_entity_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new("shader-pack block-entity sources", &[storage(5, true)])
}

/// Group 1: the jobs, the Iris vertex output and the dispatch's parameters.
pub(crate) fn output_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "shader-pack actor skinning output",
        &[
            storage(0, true),
            storage(1, false),
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(SKIN_PARAMS_BYTES),
                },
                count: None,
            },
        ],
    )
}

/// The two cached kernels (rig skinning and block-entity conversion) and their layouts.
#[derive(Resource)]
pub(crate) struct ActorSkinPipelines {
    rig: CachedComputePipelineId,
    block_entities: CachedComputePipelineId,
    rig_layout: BindGroupLayout,
    block_entity_layout: BindGroupLayout,
    output_layout: BindGroupLayout,
}

impl FromWorld for ActorSkinPipelines {
    fn from_world(world: &mut World) -> Self {
        let cache = world.resource::<PipelineCache>();
        let pipeline = |label: &'static str, source, entry: &'static str| {
            cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some(label.into()),
                layout: vec![source, output_layout()],
                shader: SKIN_SHADER,
                entry_point: Some(entry.into()),
                ..default()
            })
        };
        Self {
            rig: pipeline("shader-pack actor skinning", rig_layout(), "skin_rig"),
            block_entities: pipeline(
                "shader-pack block-entity conversion",
                block_entity_layout(),
                "convert_block_entities",
            ),
            rig_layout: cache.get_bind_group_layout(&rig_layout()),
            block_entity_layout: cache.get_bind_group_layout(&block_entity_layout()),
            output_layout: cache.get_bind_group_layout(&output_layout()),
        }
    }
}

/// The compiled kernels and their bind group layouts, once the pipeline cache has them.
#[derive(Clone, Copy)]
pub(crate) struct SkinKernels<'a> {
    pub rig: &'a ComputePipeline,
    pub block_entities: &'a ComputePipeline,
    pub rig_layout: &'a BindGroupLayout,
    pub block_entity_layout: &'a BindGroupLayout,
    pub output_layout: &'a BindGroupLayout,
}

impl ActorSkinPipelines {
    /// Both kernels, or `None` while either is still compiling.
    pub(crate) fn kernels<'a>(&'a self, cache: &'a PipelineCache) -> Option<SkinKernels<'a>> {
        Some(SkinKernels {
            rig: cache.get_compute_pipeline(self.rig)?,
            block_entities: cache.get_compute_pipeline(self.block_entities)?,
            rig_layout: &self.rig_layout,
            block_entity_layout: &self.block_entity_layout,
            output_layout: &self.output_layout,
        })
    }
}
