//! Texture face addresses and modulation preserve original GUI mesh geometry.

use std::sync::Arc;

use ui::UiMesh;

use ui::IconRef;

/// How a cube sheet's texels cover: fully, with cut-out holes, or partly see-through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui_runtime::presentation) enum SheetCoverage {
    Opaque,
    Cutout,
    Translucent,
}

/// The coverage of a sheet's texels.
pub(in crate::ui_runtime::presentation) fn sheet_coverage(rgba8: &[u8]) -> SheetCoverage {
    let mut coverage = SheetCoverage::Opaque;
    for pixel in rgba8.as_chunks::<4>().0 {
        match pixel[3] {
            255 => {}
            0 => coverage = SheetCoverage::Cutout,
            _ => return SheetCoverage::Translucent,
        }
    }
    coverage
}

/// Special translucent GUI tessellators (for example beacon) are not ordinary cubes.
pub(in crate::ui_runtime::presentation) fn ordinary_cube_sheet(rgba8: &[u8]) -> bool {
    sheet_coverage(rgba8) == SheetCoverage::Opaque
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
            vertex.style_flags |= u16::from(ui::UI_STYLE_GLINT);
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

#[cfg(test)]
mod tests {
    use super::{SheetCoverage, sheet_coverage};

    #[test]
    fn coverage_separates_opaque_cut_out_and_translucent_sheets() {
        assert_eq!(
            sheet_coverage(&[9, 9, 9, 255, 1, 1, 1, 255]),
            SheetCoverage::Opaque
        );
        assert_eq!(
            sheet_coverage(&[9, 9, 9, 255, 0, 0, 0, 0]),
            SheetCoverage::Cutout
        );
        assert_eq!(
            sheet_coverage(&[9, 9, 9, 0, 1, 1, 1, 128]),
            SheetCoverage::Translucent
        );
    }
}
