//! Terrain geometry for shader packs: Cinnabar's packed chunk arenas re-expanded into the vertex
//! stream Iris gbuffers and shadow programs read.
//!
//! # Why expansion
//! Packs read per-vertex attributes at the fixed locations `shaderpack::ProgramReflection`
//! assigns, through a translated program whose four bind groups (uniforms, textures, images,
//! buffers) are all taken. Pulling vertices inside each pack program would need a fifth group or
//! naga surgery on every program, and would re-run Cinnabar's decode per vertex per pass
//! (camera and shadow). Instead three compute kernels (`expand_cubes`, `expand_models`,
//! `expand_liquids`) call the packed renderer's own decoders (`cube_quad`, `model_quad`,
//! `liquid_record_corner`, `greedy_uv`, the biome tint and light helpers) once when a section's
//! allocation changes, and write ordinary vertices that any pack pipeline consumes unmodified.
//! A greedy quad becomes one face per covered block, as Java tessellates, because one 2D atlas
//! cannot repeat a sprite across a merged face.
//!
//! # Host contract
//! - Add [`ShaderpackGeometryPlugin`] (and [`crate::shaderpack::atlas::ShaderpackAtlasPlugin`]),
//!   then set [`ShaderpackGeometrySettings::enabled`] in the render world while a pack draws;
//!   disabled, the adapter releases its expanded and transient buffers. Set `separate_ao`/`old_lighting` from the
//!   pack's `separateAo`/`oldLighting`.
//! - Pipelines: build each gbuffers/shadow program's vertex state with [`vertex_buffer_layouts`]
//!   from `ProgramReflection::vertex_inputs` and the pack's [`TerrainLayout::compact`] layout
//!   (set as [`ShaderpackGeometrySettings::layout`]). Geometry winds counter-clockwise from the
//!   front in world space, every two-sided face carries an explicit back face, and grass-side
//!   overlays are separate coplanar faces drawn after their base, so draw every category with
//!   back-face culling and a less-or-equal depth test, as Java does.
//! - Uniforms: vertex positions are relative to [`ShaderpackGeometry::origin`] (packed
//!   layouts store them relative to each section and add the section's origin per draw).
//!   Set the pack's `chunkOffset` to [`ShaderpackGeometry::chunk_offset`] and fold the same
//!   translation into `gl_ModelViewMatrix` (`iris_ModelViewMat = gbufferModelView *
//!   translate(chunkOffset)`), so `vaPosition + chunkOffset` and `gl_ModelViewMatrix *
//!   gl_Vertex` mean what Iris gives. Set `iris_TextureMat[1]` to [`LIGHTMAP_TEXTURE_MATRIX`]:
//!   the lightmap attribute holds Java's raw `light × 16` values, which that matrix maps to
//!   `(light × 16 + 8) / 256`; `iris_TextureMat[0]` is the identity.
//! - Textures: bind `ShaderpackAtlas::{color, normals, specular}` as the pack's
//!   `gtexture`/`texture`/`tex`, `normals` and `specular`.
//! - Drawing: inside a pass holding the program's pipeline and bind groups, call
//!   [`ShaderpackGeometry::draw`] per [`GeometryCategory`] (Iris `gbuffers_terrain` for solid and
//!   cutout, `gbuffers_water` for translucent) with the camera's render view entity. Camera
//!   draws honour the packed renderer's frustum and cave culling and leave out solid and cutout
//!   faces the camera sees only from behind, as Sodium's block face culling does. Shadow
//!   passes draw the view's shadow list, which `ShaderpackGeometry::prepare_shadow_view` builds
//!   from the pack's shadow culling (see `shadow_cull`).
//! - Block ids: insert [`ShaderpackBlockIds`] to supply `mc_Entity` ids; without it they read -1.
//!
//! # Memory
//! One face is four vertices of the pack's layout: 8 bytes of packed position (with the
//! lightmap coordinates in its `w`) plus 4 bytes per other field its terrain programs read
//! (at most 36 bytes). Faces live in vertex pages
//! no larger than one storage binding, within [`ShaderpackGeometrySettings::budget_bytes`];
//! sections that do not fit wait, undrawn, until space frees (a warning is logged once).
//! Expansion is bounded to [`FRAME_EXPANSION_FACES`] per frame, except after an origin move,
//! which re-expands terrain within the device’s job-buffer limits.

mod allocator;
mod block_ids;
mod draw;
mod expansion_buffers;
mod gpu;
pub(crate) mod jobs;
mod pipelines;
mod residency;
pub(super) mod shadow_cull;
mod vertex;

use bevy::math::Mat4;
use bevy::prelude::{App, Commands, IntoScheduleConfigs, Plugin, Res, World};
use bevy::render::{Render, RenderApp, RenderSystems};

pub use block_ids::{BlockEntityIdFn, ShaderpackBlockIds, UNASSIGNED_BLOCK_ID};
pub(crate) use gpu::prepare_shaderpack_geometry;
pub use gpu::{FRAME_EXPANSION_FACES, ShaderpackGeometry, ShaderpackGeometrySettings};
pub use jobs::GeometryCategory;
pub use residency::{DEFAULT_GEOMETRY_BUDGET_BYTES, device_geometry_budget};
pub(crate) use shadow_cull::{ShadowCulling, ShadowFrustum};
#[cfg(test)]
pub(crate) use vertex::{BLOCK_ID, LIGHTMAP, NORMAL, POSITION, UV0};
pub(crate) use vertex::{
    CONSTANT_STREAM_WORDS, f32_to_half, host_vertex, substitute_layout_constants,
    textured_host_vertex,
};
pub use vertex::{
    CONSTANT_VERTEX_BUFFER, DRAW_ORIGIN_VERTEX_BUFFER, GEOMETRY_VERTEX_BUFFER, IRIS_VERTEX_STRIDE,
    TerrainLayout, VertexLayoutError, vertex_buffer_layouts,
};

/// `gl_TextureMatrix[1]` of Java terrain: maps raw `light × 16` lightmap coordinates to
/// `(light × 16 + 8) / 256`, the centre of the lightmap texel.
pub const LIGHTMAP_TEXTURE_MATRIX: Mat4 = Mat4::from_cols_array(&[
    1.0 / 256.0,
    0.0,
    0.0,
    0.0,
    0.0,
    1.0 / 256.0,
    0.0,
    0.0,
    0.0,
    0.0,
    1.0 / 256.0,
    0.0,
    8.0 / 256.0,
    8.0 / 256.0,
    8.0 / 256.0,
    1.0,
]);

/// Expands packed terrain for shader packs; see the module documentation.
pub struct ShaderpackGeometryPlugin;

impl Plugin for ShaderpackGeometryPlugin {
    fn build(&self, app: &mut App) {
        pipelines::load_shaders(app);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<ShaderpackGeometrySettings>()
            .add_systems(
                Render,
                crate::chunk::shaderpack_sources::after_packed_chunk_upload(
                    (prepare_geometry_resources, gpu::prepare_shaderpack_geometry)
                        .chain()
                        .after(RenderSystems::PrepareViews)
                        .before(RenderSystems::Queue),
                ),
            )
            .add_systems(bevy::render::RenderStartup, install_expansion_recording);
    }
}

/// Creates terrain state and queues its kernels only after a pack enables expansion.
pub(crate) fn prepare_geometry_resources(
    mut commands: Commands,
    settings: Res<ShaderpackGeometrySettings>,
    geometry: Option<Res<ShaderpackGeometry>>,
    pipelines: Option<Res<pipelines::ExpansionPipelines>>,
) {
    if settings.enabled && (geometry.is_none() || pipelines.is_none()) {
        commands.init_resource::<pipelines::ExpansionPipelines>();
        commands.init_resource::<ShaderpackGeometry>();
    }
}

/// Records expansion in the render graph after staged buffer uploads and before any camera.
fn install_expansion_recording(world: &mut World) {
    let _ = world.try_schedule_scope(bevy::render::renderer::RenderGraph, |_, schedule| {
        schedule.add_systems(
            gpu::record_geometry_expansion
                .after(crate::upload_staging::BufferUploads)
                .before(bevy::core_pipeline::schedule::camera_driver)
                .in_set(bevy::render::renderer::RenderGraphSystems::Render),
        );
    });
}

#[cfg(test)]
mod expansion_draw_tests;
#[cfg(test)]
mod expansion_tests;
