//! Actor vertices retain floating-point UVs alongside the shared Iris fields.

use crate::shaderpack::geometry::{
    IRIS_VERTEX_STRIDE, TerrainLayout, VertexLayoutError, vertex_buffer_layouts,
};
use bevy::mesh::VertexBufferLayout;
use bevy::render::render_resource::VertexFormat;
use shaderpack::{VertexAttribute, VertexInput};

pub(crate) const ACTOR_UV_WORD: u32 = (IRIS_VERTEX_STRIDE / 4) as u32;
pub(crate) const ACTOR_MID_UV_WORD: u32 = ACTOR_UV_WORD + 2;
pub(crate) const ACTOR_VERTEX_WORDS: u32 = ACTOR_MID_UV_WORD + 2;
pub(crate) const ACTOR_VERTEX_STRIDE: u64 = ACTOR_VERTEX_WORDS as u64 * 4;

/// Binds actor UV attributes as full floats, preserving wrapping after interpolation.
pub(crate) fn vertex_layouts(
    inputs: &[VertexInput],
) -> Result<Vec<VertexBufferLayout>, VertexLayoutError> {
    let mut layouts = vertex_buffer_layouts(inputs, &TerrainLayout::WIDE)?;
    let geometry = &mut layouts[crate::shaderpack::geometry::GEOMETRY_VERTEX_BUFFER as usize];
    geometry.array_stride = ACTOR_VERTEX_STRIDE;
    for input in inputs {
        let word = match input.attribute {
            VertexAttribute::TexCoord => ACTOR_UV_WORD,
            VertexAttribute::MidTexCoord => ACTOR_MID_UV_WORD,
            _ => continue,
        };
        for attribute in &mut geometry.attributes {
            if attribute.shader_location == input.location {
                attribute.offset = u64::from(word) * 4;
                attribute.format = VertexFormat::Float32x2;
            }
        }
    }
    Ok(layouts)
}
