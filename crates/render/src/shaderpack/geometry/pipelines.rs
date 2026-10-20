//! Expansion kernels: shader registration, generated constants, layouts and cached pipelines.

use std::num::NonZeroU64;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::prelude::{App, FromWorld, Handle, Resource, Shader, World, default};
use bevy::render::render_resource::{
    BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType, BufferBindingType,
    CachedComputePipelineId, ComputePipelineDescriptor, PipelineCache, ShaderStages,
};

use super::jobs::{JOB_BACK_ONLY, JOB_OVERLAY_FACE, JOB_REVERSE_FACE, JobKind};
use super::vertex;
use crate::shaderpack::atlas;

const EXPAND_COMMON_SHADER: Handle<Shader> = uuid_handle!("6a0f5d0e-4c3b-4f43-9b7e-2f0f7c7d1a01");
const EXPAND_CUBES_SHADER: Handle<Shader> = uuid_handle!("6a0f5d0e-4c3b-4f43-9b7e-2f0f7c7d1a02");
const EXPAND_MODELS_SHADER: Handle<Shader> = uuid_handle!("6a0f5d0e-4c3b-4f43-9b7e-2f0f7c7d1a03");
const EXPAND_LIQUIDS_SHADER: Handle<Shader> = uuid_handle!("6a0f5d0e-4c3b-4f43-9b7e-2f0f7c7d1a04");

/// Invocations per expansion workgroup, substituted into the kernels.
pub(crate) const EXPAND_WORKGROUP_SIZE: u32 = 64;
/// Bytes of the kernels' `ExpandParams` uniform.
pub(crate) const EXPAND_PARAMS_BYTES: u64 = 64;

/// Resolves every generated constant of an expansion module: the packed-material constants,
/// the expanded vertex layout, the atlas remap slots and the job flags.
pub(crate) fn expansion_shader_source(source: &str) -> String {
    let source = vertex::substitute_layout_constants(&crate::material_shader::source(source));
    let page_slots = atlas::static_sprite_slot(1, 0) - atlas::static_sprite_slot(0, 0);
    source
        .replace(
            "ATLAS_HEADER_SLOTS",
            &format!("{}u", atlas::ATLAS_HEADER_SLOTS),
        )
        .replace(
            "ATLAS_STATIC_SLOT_BASE",
            &format!("{}u", atlas::static_sprite_slot(0, 0)),
        )
        .replace(
            "ATLAS_OPAQUE_SLOT_BASE",
            &format!("{}u", atlas::opaque_sprite_slot(0, 0)),
        )
        .replace(
            "ATLAS_ANIMATION_SLOT_BASE",
            &format!("{}u", atlas::animation_sprite_slot(0)),
        )
        .replace("ATLAS_PAGE_SLOTS", &format!("{page_slots}u"))
        .replace(
            "GENERATED_EXPAND_WORKGROUP_SIZE",
            &format!("{EXPAND_WORKGROUP_SIZE}u"),
        )
        .replace("JOB_OVERLAY_FACE", &format!("{JOB_OVERLAY_FACE}u"))
        .replace("JOB_REVERSE_FACE", &format!("{JOB_REVERSE_FACE}u"))
        .replace("JOB_BACK_ONLY", &format!("{JOB_BACK_ONLY}u"))
        .replace(
            "MODEL_QUAD_TWO_SIDED_FLAG",
            &format!("{}u", assets::MODEL_QUAD_FLAG_TWO_SIDED),
        )
}

fn shader(source: &str, path: impl Into<String>) -> Shader {
    crate::shader_safety::from_wesl(expansion_shader_source(source), path)
}

/// Registers the expansion modules with Bevy's shader composer.
pub(crate) fn load_shaders(app: &mut App) {
    load_internal_asset!(app, EXPAND_COMMON_SHADER, "expand_common.wesl", shader);
    load_internal_asset!(app, EXPAND_CUBES_SHADER, "expand_cubes.wesl", shader);
    load_internal_asset!(app, EXPAND_MODELS_SHADER, "expand_models.wesl", shader);
    load_internal_asset!(app, EXPAND_LIQUIDS_SHADER, "expand_liquids.wesl", shader);
}

/// Packed-chunk bindings (group 0) each kernel reads, at the chunk layout's numbers.
pub(crate) fn source_bindings(kind: JobKind) -> &'static [u32] {
    // 1 quads, 2 origins, 3 materials, 7 biome records, 8 biome tints, 12 model templates,
    // 13 geometry streams; the animation clock and the biome query tables follow.
    match kind {
        JobKind::Cube => &[1, 2, 3, 7, 8, 13],
        JobKind::Model => &[2, 3, 7, 8, 12, 13],
        JobKind::Liquid => &[2, 3, 7, 8, 13],
    }
}

/// The packed renderer's chunk binding of the animation clock uniform.
pub(crate) const ANIMATION_CLOCK_BINDING: u32 = 11;

/// Group 0: the packed-chunk storage buffers `kind` reads, the animation clock and the biome
/// query tables, at the packed renderer's chunk bindings.
pub(crate) fn source_layout(kind: JobKind) -> BindGroupLayoutDescriptor {
    let mut entries: Vec<_> = source_bindings(kind)
        .iter()
        .map(|&binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::COMPUTE,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect();
    entries.extend(
        [
            ANIMATION_CLOCK_BINDING,
            crate::material_shader::BIOME_QUERY_TABLES_BINDING,
        ]
        .map(|binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::COMPUTE,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }),
    );
    let label = match kind {
        JobKind::Cube => "shader-pack cube expansion sources",
        JobKind::Model => "shader-pack model expansion sources",
        JobKind::Liquid => "shader-pack liquid expansion sources",
    };
    BindGroupLayoutDescriptor::new(label, &entries)
}

/// Group 1: job words, the expanded vertices, the atlas remap and the parameters.
pub(crate) fn output_layout() -> BindGroupLayoutDescriptor {
    let storage = |binding, read_only| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::COMPUTE,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    BindGroupLayoutDescriptor::new(
        "shader-pack expansion output",
        &[
            storage(0, true),
            storage(1, false),
            storage(2, true),
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(EXPAND_PARAMS_BYTES),
                },
                count: None,
            },
        ],
    )
}

/// Storage buffers one kernel binds per stage, checked against the device before use.
pub(crate) fn storage_buffers_per_stage(kind: JobKind) -> u32 {
    source_bindings(kind).len() as u32 + 3
}

/// The three cached expansion pipelines, indexed by [`JobKind`].
#[derive(Resource)]
pub(crate) struct ExpansionPipelines {
    pub(crate) pipelines: [CachedComputePipelineId; 3],
}

impl FromWorld for ExpansionPipelines {
    fn from_world(world: &mut World) -> Self {
        let cache = world.resource::<PipelineCache>();
        let pipeline = |kind: JobKind| {
            let (label, shader, entry) = match kind {
                JobKind::Cube => (
                    "shader-pack cube expansion",
                    EXPAND_CUBES_SHADER,
                    "expand_cubes",
                ),
                JobKind::Model => (
                    "shader-pack model expansion",
                    EXPAND_MODELS_SHADER,
                    "expand_models",
                ),
                JobKind::Liquid => (
                    "shader-pack liquid expansion",
                    EXPAND_LIQUIDS_SHADER,
                    "expand_liquids",
                ),
            };
            cache.queue_compute_pipeline(ComputePipelineDescriptor {
                label: Some(label.into()),
                layout: vec![source_layout(kind), output_layout()],
                shader,
                entry_point: Some(entry.into()),
                ..default()
            })
        };
        Self {
            pipelines: super::jobs::JOB_KINDS.map(pipeline),
        }
    }
}
