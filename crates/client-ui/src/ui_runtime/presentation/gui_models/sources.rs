//! Texture face addresses and modulation preserve original GUI mesh geometry.

use std::{collections::HashMap, sync::Arc};

use assets::{BlockFace, BlockVisualId, IconSprite, NetworkIdMode, RuntimeAssets};
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

/// Normalizes non-blended cube faces like the thumbnail baker. Fractional blended faces or
/// unresolved alpha modes keep the thumbnail; binary alpha is independent of material mode.
pub(super) fn gui_cube_sheet(
    world: &RuntimeAssets,
    visual: BlockVisualId,
    sprite: &IconSprite,
) -> Option<IconSprite> {
    let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
    if [sprite.width, sprite.height] != [width, height]
        || sprite.rgba8.len() != usize::from(width) * usize::from(height) * 4
    {
        return None;
    }
    if sheet_coverage(&sprite.rgba8) != SheetCoverage::Translucent {
        return Some(sprite.clone());
    }
    let block = world.resolve(NetworkIdMode::Sequential, visual.0);
    if !block.is_known() {
        return None;
    }
    let flags = BlockFace::ALL.map(|face| {
        let id = block.face(face).material_id();
        (id != assets::DIAGNOSTIC_MATERIAL)
            .then(|| world.materials().get(id as usize))
            .flatten()
            .map(|material| material.flags)
    });
    let mut normalized = sprite.clone();
    normalize_sheet_alpha(&mut normalized, flags);
    (sheet_coverage(&normalized.rgba8) != SheetCoverage::Translucent).then_some(normalized)
}

/// Matches the block GUI baker's cutoff independently for each non-blended material face.
/// Blended faces keep their source alpha; RGB and unsupported material references stay unchanged.
pub(super) fn normalize_sheet_alpha(sprite: &mut assets::IconSprite, flags: [Option<u32>; 6]) {
    if [sprite.width, sprite.height] != assets::BLOCK_ITEM_SHEET_SIZE
        || sprite.rgba8.len() != usize::from(sprite.width) * usize::from(sprite.height) * 4
    {
        return;
    }
    let cutoff = flags
        .map(|flags| flags.is_some_and(|flags| flags & assets::MATERIAL_FLAG_ALPHA_BLEND == 0));
    if !cutoff.contains(&true) {
        return;
    }
    let side = usize::from(assets::BLOCK_ITEM_FACE_SIDE);
    let columns = usize::from(assets::BLOCK_ITEM_SHEET_GRID[0]);
    let width = usize::from(sprite.width);
    for (pixel, texel) in Arc::make_mut(&mut sprite.rgba8)
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        let face = (pixel / width / side) * columns + pixel % width / side;
        if cutoff[face] {
            texel[3] = if texel[3] >= assets::gui_item::GUI_BLOCK_ALPHA_THRESHOLD {
                u8::MAX
            } else {
                0
            };
        }
    }
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

/// Tinted and glinting copies of model meshes, reused while frames keep drawing them.
#[derive(Default)]
pub(super) struct ModulatedMeshes {
    /// By source mesh address, tint and glint.
    entries: HashMap<(usize, [u8; 4], bool), ModulatedMesh>,
}

struct ModulatedMesh {
    /// Keeps the source allocation alive, so its address keys no other mesh.
    source: Arc<UiMesh>,
    mesh: Option<Arc<UiMesh>>,
    drawn: bool,
}

impl ModulatedMeshes {
    /// [`modulated`] for `mesh`, sharing the copy made for the same source, tint and glint.
    pub(super) fn get(
        &mut self,
        mesh: &Arc<UiMesh>,
        color: [u8; 4],
        glint: bool,
    ) -> Option<Arc<UiMesh>> {
        if color == [255; 4] && !glint {
            return modulated(mesh, color, glint);
        }
        let entry = self
            .entries
            .entry((Arc::as_ptr(mesh).addr(), color, glint))
            .and_modify(|entry| {
                if !Arc::ptr_eq(&entry.source, mesh) {
                    *entry = ModulatedMesh::new(mesh, color, glint);
                }
            })
            .or_insert_with(|| ModulatedMesh::new(mesh, color, glint));
        entry.drawn = true;
        entry.mesh.clone()
    }

    /// Forgets the copies no frame drew since the last call.
    pub(super) fn retain_drawn(&mut self) {
        self.entries
            .retain(|_, entry| std::mem::replace(&mut entry.drawn, false));
    }
}

impl ModulatedMesh {
    fn new(source: &Arc<UiMesh>, color: [u8; 4], glint: bool) -> Self {
        Self {
            source: Arc::clone(source),
            mesh: modulated(source, color, glint),
            drawn: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{ModulatedMeshes, SheetCoverage, modulated, normalize_sheet_alpha, sheet_coverage};

    #[test]
    fn modulated_meshes_share_copies_until_source_tint_or_glint_changes() {
        let icon = ui::IconRef {
            color_mask: None,
            page: 3,
            uv: [0, 0, 16, 16],
            glint: false,
        };
        let source = super::super::item_gui::cube([icon; 6]).unwrap();
        let other = super::super::item_gui::cube([icon; 6]).unwrap();
        let faded = [255, 255, 255, 128];
        let mut meshes = ModulatedMeshes::default();
        let first = meshes.get(&source, faded, false).unwrap();
        assert_eq!(first, modulated(&source, faded, false).unwrap());
        assert!(Arc::ptr_eq(
            &first,
            &meshes.get(&source, faded, false).unwrap()
        ));
        let glinting = meshes.get(&source, faded, true).unwrap();
        assert!(!Arc::ptr_eq(&first, &glinting));
        assert_eq!(glinting, modulated(&source, faded, true).unwrap());
        let tinted = meshes.get(&source, [255, 0, 0, 255], false).unwrap();
        assert_eq!(tinted, modulated(&source, [255, 0, 0, 255], false).unwrap());
        assert!(
            !Arc::ptr_eq(&first, &meshes.get(&other, faded, false).unwrap()),
            "each source mesh has its own copy"
        );
        assert!(Arc::ptr_eq(
            &meshes.get(&source, [255; 4], false).unwrap(),
            &source
        ));
        meshes.retain_drawn();
        assert!(Arc::ptr_eq(
            &first,
            &meshes.get(&source, faded, false).unwrap()
        ));
        meshes.retain_drawn();
        meshes.retain_drawn();
        assert!(
            !Arc::ptr_eq(&first, &meshes.get(&source, faded, false).unwrap()),
            "a copy no frame drew is made again"
        );
    }

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
    #[test]
    fn cube_face_materials_keep_blend_alpha_and_threshold_cutout_texels() {
        let [width, height] = assets::BLOCK_ITEM_SHEET_SIZE;
        let threshold = assets::gui_item::GUI_BLOCK_ALPHA_THRESHOLD;
        let alphas = [0, threshold - 1, threshold, u8::MAX];
        let source: Vec<_> = (0..usize::from(width) * usize::from(height))
            .flat_map(|pixel| [10, 20, 30, alphas[pixel % alphas.len()]])
            .collect();
        let mut sprite = assets::IconSprite {
            width,
            height,
            rgba8: source.clone().into(),
        };
        let flags = assets::BlockFace::ALL.map(|face| {
            Some(match face {
                assets::BlockFace::West => assets::MATERIAL_FLAG_ALPHA_BLEND,
                assets::BlockFace::East => 0,
                _ => assets::MATERIAL_FLAG_ALPHA_CUTOUT,
            })
        });
        normalize_sheet_alpha(&mut sprite, flags);
        let side = usize::from(assets::BLOCK_ITEM_FACE_SIDE);
        let columns = usize::from(assets::BLOCK_ITEM_SHEET_GRID[0]);
        for (index, (original, actual)) in source
            .as_chunks::<4>()
            .0
            .iter()
            .zip(sprite.rgba8.as_chunks::<4>().0)
            .enumerate()
        {
            let face =
                (index / usize::from(width) / side) * columns + index % usize::from(width) / side;
            assert_eq!(actual[..3], original[..3]);
            assert_eq!(
                actual[3],
                if assets::BlockFace::ALL[face] == assets::BlockFace::West {
                    original[3]
                } else if original[3] >= threshold {
                    255
                } else {
                    0
                }
            );
        }
    }
}
