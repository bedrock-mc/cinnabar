//! Texture face addresses and modulation preserve original GUI mesh geometry.

use std::sync::Arc;

use ui::UiMesh;

use super::IconRef;

/// Special translucent GUI tessellators (for example beacon) are not ordinary cubes.
pub(in crate::ui_runtime::presentation) fn ordinary_cube_sheet(rgba8: &[u8]) -> bool {
    rgba8.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255)
}

pub(in crate::ui_runtime::presentation) fn sheet_faces(icon: IconRef) -> [IconRef; 6] {
    let side = assets::BLOCK_ITEM_FACE_SIDE;
    let columns = usize::from(assets::BLOCK_ITEM_SHEET_GRID[0]);
    std::array::from_fn(|face| {
        let left = icon.uv[0] + (face % columns) as u16 * side;
        let top = icon.uv[1] + (face / columns) as u16 * side;
        IconRef {
            uv: [left, top, left + side, top + side],
            ..icon
        }
    })
}

pub(super) fn modulated(mesh: &Arc<UiMesh>, color: [u8; 4], glint: bool) -> Option<Arc<UiMesh>> {
    if color == [255; 4] && !glint {
        return Some(Arc::clone(mesh));
    }
    let mut vertices = mesh.vertices().to_vec();
    for vertex in &mut vertices {
        for (channel, factor) in vertex.color.iter_mut().zip(color) {
            *channel = (u16::from(*channel) * u16::from(factor) / 255) as u8;
        }
        if glint {
            vertex.style_flags |= ui::UI_STYLE_GLINT;
        }
    }
    UiMesh::new(
        vertices.into(),
        mesh.indices().into(),
        mesh.batches().into(),
    )
    .ok()
    .map(Arc::new)
}
