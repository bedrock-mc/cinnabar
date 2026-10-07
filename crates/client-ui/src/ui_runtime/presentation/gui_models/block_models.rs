//! Native geometry for block items whose carrier icon is a model thumbnail (slabs, stairs, walls,
//! fences), so they draw at the control's physical size like ordinary cubes.

use std::sync::Arc;

use assets::gui_item::{block_item_quads, material_tile};
use assets::{
    BlockFace, BlockVisualId, Material, ModelQuad, ModelTemplate, NetworkIdMode, RuntimeAssets,
    TexturePage, VisualKind,
};
use ui::UiMesh;

use super::{atlas::Atlas, item_gui};

/// One block state's shape and the world tables it indexes.
struct Shape<'a> {
    kind: VisualKind,
    faces: [u32; 6],
    template: Option<u32>,
    templates: &'a [ModelTemplate],
    quads: &'a [ModelQuad],
    materials: &'a [Material],
    pages: &'a [TexturePage],
}

/// The world state's GUI quads over their full-resolution tiles. `None` keeps the thumbnail: an
/// unknown state, a refused or blended material, a UV wrapping past its tile, or a full atlas.
pub(super) fn mesh(
    world: &RuntimeAssets,
    visual: BlockVisualId,
    atlas: &mut Atlas,
) -> Option<Arc<UiMesh>> {
    if world.is_diagnostic() || visual.0 as usize >= world.visual_count() {
        return None;
    }
    let block = world.resolve(NetworkIdMode::Sequential, visual.0);
    if !block.is_known() {
        return None;
    }
    let shape = Shape {
        kind: block.kind(),
        faces: BlockFace::ALL.map(|face| block.face(face).material_id()),
        template: block.model_template(),
        templates: world.model_templates(),
        quads: world.model_quads(),
        materials: world.materials(),
        pages: world.texture_pages(),
    };
    shaped(&shape, atlas)
}

fn shaped(shape: &Shape<'_>, atlas: &mut Atlas) -> Option<Arc<UiMesh>> {
    let quads = block_item_quads(
        shape.kind,
        shape.faces,
        shape.template,
        shape.templates,
        shape.quads,
    )
    .ok()?;
    let mut placed = Vec::with_capacity(quads.len());
    for quad in quads {
        let (tile, side, blend) = material_tile(shape.materials, quad.material, |page| {
            shape.pages.get(page as usize).map(|page| &page.texture)
        })
        .ok()?;
        if blend {
            return None;
        }
        let side = u16::try_from(side).ok()?;
        placed.push((quad, atlas.insert([side, side], tile).ok()?));
    }
    item_gui::block_model(&placed)
}

#[cfg(test)]
mod tests;
