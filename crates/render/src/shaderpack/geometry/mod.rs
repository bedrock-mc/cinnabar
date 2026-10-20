//! Terrain geometry for shader packs: Cinnabar's packed chunk arenas re-expanded into the vertex
//! stream Iris gbuffers and shadow programs read.
//!
//! This part holds the CPU side: the vertex layouts pack pipelines read
//! ([`vertex_buffer_layouts`], [`TerrainLayout`]), the expansion jobs a section's packed
//! quads become (`jobs`), the per-material `mc_Entity` ids ([`ShaderpackBlockIds`]) and the
//! residency of expanded sections in vertex pages under a memory budget (`residency`).
//!
//! # Memory
//! One face is four vertices of the pack's layout: 8 bytes of packed position (with the
//! lightmap coordinates in its `w`) plus 4 bytes per other field its terrain programs read
//! (at most 36 bytes). Faces live in vertex pages no larger than one storage binding, within
//! the budget; sections that do not fit wait, undrawn, until space frees.

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "consumed by the GPU expansion and draw lists")
)]
mod allocator;
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "consumed by the GPU expansion and draw lists")
)]
mod block_ids;
#[expect(dead_code, reason = "consumed by the GPU expansion and draw lists")]
pub(crate) mod jobs;
#[expect(dead_code, reason = "consumed by the GPU expansion and draw lists")]
mod residency;
#[expect(dead_code, reason = "consumed by the GPU expansion and draw lists")]
mod vertex;

use bevy::math::Mat4;

pub use block_ids::{BlockEntityIdFn, ShaderpackBlockIds, UNASSIGNED_BLOCK_ID};
pub use jobs::GeometryCategory;
pub use residency::DEFAULT_GEOMETRY_BUDGET_BYTES;
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
