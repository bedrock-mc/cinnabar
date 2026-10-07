//! Native inventory geometry, rasterized by the UI pass at the current framebuffer resolution.

use std::sync::Arc;

use assets::gui_item::CUBE_FACES;
use ui::{UiBlendMode, UiMesh, UiMeshBatch, UiMeshVertex};

use super::IconRef;

mod block_model;
mod shield;

pub(super) use block_model::mesh as block_model;

pub(super) fn shield(
    geometry: &assets::EntityGeometry,
    texture: &assets::EquipmentTexture,
    texture_ref: IconRef,
) -> Option<Arc<UiMesh>> {
    shield::mesh(geometry, texture, texture_ref)
}

/// The native item renderer's design-pixel frame, independent of texture and display resolution.
pub(super) use assets::gui_item::{GUI_ITEM_SIDE, SHIELD_IDENTIFIER};

/// Ordinary opaque cubes. `faces` retains the carried texture in `BlockFace::ALL` order.
/// Other block shapes (slabs, stairs, fences) draw through [`block_model`].
pub(super) fn cube(faces: [IconRef; 6]) -> Option<Arc<UiMesh>> {
    let mut vertices = Vec::with_capacity(CUBE_FACES.len() * 4);
    let mut indices = Vec::with_capacity(CUBE_FACES.len() * 6);
    let mut batches = Vec::with_capacity(CUBE_FACES.len());
    for (face, positions, uvs, brightness) in CUBE_FACES {
        let shade = (brightness * f32::from(u8::MAX)) as u8;
        let texture = faces[face as usize];
        if texture.uv[2] <= texture.uv[0] || texture.uv[3] <= texture.uv[1] {
            return None;
        }
        let start = vertices.len() as u32;
        for (point, uv) in positions.into_iter().zip(uvs) {
            vertices.push(vertex(
                cube_project(point),
                atlas_uv(texture, uv)?,
                [shade, shade, shade, 255],
                texture.glint,
                false,
            ));
        }
        let begin = indices.len() as u32;
        indices.extend([0, 1, 2, 0, 2, 3].map(|i| start + i));
        batches.push(batch(texture.page, begin..indices.len() as u32, None));
    }
    UiMesh::new(vertices.into(), indices.into(), batches.into())
        .ok()
        .map(Arc::new)
}

fn cube_project(point: [f32; 3]) -> [f32; 2] {
    let [x, y, _] = assets::gui_item::project_cube(point);
    [x / GUI_ITEM_SIDE, y / GUI_ITEM_SIDE]
}

fn vertex(
    position: [f32; 2],
    uv: [f32; 2],
    color: [u8; 4],
    glint: bool,
    alpha_test: bool,
) -> UiMeshVertex {
    UiMeshVertex {
        position,
        clip_z: 0.0,
        clip_w: 1.0,
        uv,
        color,
        model_light: 1.0,
        overlay_color: [0.0; 4],
        style_flags: if glint { ui::UI_STYLE_GLINT } else { 0 },
        alpha_test,
    }
}

fn batch(
    texture_page: u16,
    index_range: std::ops::Range<u32>,
    alpha_cutoff: Option<f32>,
) -> UiMeshBatch {
    UiMeshBatch {
        texture_page,
        index_range,
        blend: UiBlendMode::Alpha,
        depth_test: false,
        depth_write: false,
        alpha_cutoff,
    }
}

fn atlas_uv(texture: IconRef, [u, v]: [f32; 2]) -> Option<[f32; 2]> {
    let coordinate = |low: u16, high: u16, fraction: f32| {
        let value = f32::from(low) + fraction * f32::from(high.checked_sub(low)?);
        // Keep authored sub-texel UVs (including extrusion side texel centers) intact.
        (value.is_finite() && value >= 0.0 && value <= f32::from(u16::MAX)).then_some(value)
    };
    Some([
        coordinate(texture.uv[0], texture.uv[2], u)?,
        coordinate(texture.uv[1], texture.uv[3], v)?,
    ])
}

#[cfg(test)]
mod tests;
