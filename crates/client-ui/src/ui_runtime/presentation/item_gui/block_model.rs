//! Non-cube block items (slabs, stairs, walls, fences) as GUI geometry: the quads their baked
//! thumbnail projects, drawn at the control's physical size with the item's own depth.

use std::{collections::BTreeMap, sync::Arc};

use assets::gui_item::{GuiBlockQuad, face_brightness, project_cube};
use ui::UiMesh;

use super::{GUI_ITEM_SIDE, IconRef, atlas_uv, batch, vertex};

/// Uses the same accepted alpha byte as the thumbnail baker.
const ALPHA_CUTOFF: f32 = assets::gui_item::GUI_BLOCK_ALPHA_THRESHOLD as f32 / u8::MAX as f32;

/// Each quad with its tile's place in the atlas. `None` when a UV leaves its tile, which an atlas
/// cannot wrap.
pub(in crate::ui_runtime::presentation) fn mesh(
    quads: &[(GuiBlockQuad, IconRef)],
) -> Option<Arc<UiMesh>> {
    let mut vertices = Vec::with_capacity(quads.len() * 4);
    let mut by_page: BTreeMap<u16, Vec<u32>> = BTreeMap::new();
    for (quad, texture) in quads {
        if quad
            .uvs
            .iter()
            .flatten()
            .any(|value| !(0.0..=1.0).contains(value))
        {
            return None;
        }
        let shade = (face_brightness(quad.corners) * f32::from(u8::MAX)) as u8;
        let start = vertices.len() as u32;
        for (corner, uv) in quad.corners.into_iter().zip(quad.uvs) {
            let [x, y, z] = project_cube(corner);
            let mut vertex = vertex(
                [x / GUI_ITEM_SIDE, y / GUI_ITEM_SIDE],
                atlas_uv(*texture, uv)?,
                [shade, shade, shade, 255],
                texture.glint,
                true,
            );
            vertex.clip_z = depth(z);
            vertices.push(vertex);
        }
        by_page
            .entry(texture.page)
            .or_default()
            .extend([0, 1, 2, 0, 2, 3].map(|index| start + index));
    }
    let mut indices = Vec::with_capacity(quads.len() * 6);
    let mut batches = Vec::with_capacity(by_page.len());
    for (page, page_indices) in by_page {
        let begin = indices.len() as u32;
        indices.extend(page_indices);
        let mut batch = batch(page, begin..indices.len() as u32, Some(ALPHA_CUTOFF));
        batch.depth_test = true;
        batch.depth_write = true;
        batches.push(batch);
    }
    UiMesh::new(vertices.into(), indices.into(), batches.into())
        .ok()
        .map(Arc::new)
}

/// The projection's view depth (smaller is nearer) as UI depth, where greater is nearer.
fn depth(z: f32) -> f32 {
    0.5 - z / 8.0
}

#[cfg(test)]
mod tests;
