//! Full block cubes held in hand and drawn in slots, with six independently authored faces.

use std::collections::BTreeMap;

use assets::{
    BLOCK_ITEM_FACE_SIDE, BLOCK_ITEM_SHEET_GRID, BlockFace, BlockFlags, BlockOverlay,
    DIAGNOSTIC_MATERIAL, IconSprite, ItemVisualDefinitionRoute, Material, NO_ANIMATION,
    NetworkIdMode, RuntimeAssets, RuntimeEntityAssets, TextureArray, TextureMip, VisualKind,
    VisualSupport, compose_block_item_sheet,
};

const TILE: usize = BLOCK_ITEM_FACE_SIDE as usize;

#[cfg(test)]
mod tests;

/// Composed block-item sheets and the visual IDs that select them.
pub struct BlockSheets {
    pub sheets: Vec<IconSprite>,
    /// Block visual id to its sheet index.
    pub by_visual: BTreeMap<u32, usize>,
}

/// Shares face sheets across cube items independently of their terrain occlusion and alpha.
pub fn collect(world: &RuntimeAssets, entities: &RuntimeEntityAssets) -> BlockSheets {
    let mut sheets = Vec::new();
    let mut by_materials = BTreeMap::<[u32; 6], usize>::new();
    let mut by_visual = BTreeMap::new();
    if !world.provenance().is_complete() {
        return BlockSheets { sheets, by_visual };
    }
    for definition in entities.item_visuals() {
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            continue;
        };
        let visual = block_visual.0;
        if by_visual.contains_key(&visual) || visual as usize >= world.visual_count() {
            continue;
        }
        let Some(materials) = cube_materials(world, visual) else {
            continue;
        };
        let index = match by_materials.get(&materials) {
            Some(index) => *index,
            None => {
                let Some(sheet) = compose_sheet(world, &materials) else {
                    continue;
                };
                sheets.push(sheet);
                by_materials.insert(materials, sheets.len() - 1);
                sheets.len() - 1
            }
        };
        by_visual.insert(visual, index);
    }
    BlockSheets { sheets, by_visual }
}

/// Selects exact cube geometry, including the carrier's transparent cube templates.
fn cube_materials(world: &RuntimeAssets, visual: u32) -> Option<[u32; 6]> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    let cube = match block.kind() {
        VisualKind::Cube => block.flags().contains(BlockFlags::CUBE_GEOMETRY),
        VisualKind::Model => block
            .model_template()
            .and_then(|id| world.model_templates().get(id as usize))
            .is_some_and(|template| {
                template.flags & assets::MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE != 0
            }),
        _ => false,
    };
    if !block.is_known()
        || !cube
        || block.support() != VisualSupport::Exact
        || block.animation().is_some()
    {
        return None;
    }
    let mut materials = [0; 6];
    for (slot, face) in materials.iter_mut().zip(BlockFace::ALL) {
        *slot = block.face(face).material_id();
        if *slot == DIAGNOSTIC_MATERIAL {
            return None;
        }
    }
    Some(materials)
}

/// Composes the six admitted face materials into their shared sheet layout.
fn compose_sheet(world: &RuntimeAssets, materials: &[u32; 6]) -> Option<IconSprite> {
    let mut tiles = Vec::with_capacity(BlockFace::ALL.len());
    for id in materials {
        let material = world.materials().get(*id as usize)?;
        let page = world
            .texture_pages()
            .get(material.texture.page() as usize)?;
        tiles.push(face_tile(
            material,
            &page.texture,
            page.texture
                .mips
                .iter()
                .find(|mip| mip.size as usize == TILE)?,
        )?);
    }
    compose_block_item_sheet(&tiles.try_into().ok()?)
}

/// A session cube's face sheet, including blended and cutout cube templates.
pub fn overlay_sheet(overlay: &BlockOverlay, visual: usize) -> Option<IconSprite> {
    let block = overlay.visuals.get(visual)?;
    let cube = match block.kind {
        VisualKind::Cube => block.flags.contains(BlockFlags::CUBE_GEOMETRY),
        VisualKind::Model => overlay
            .model_templates
            .get(block.model_template as usize)
            .is_some_and(|template| {
                template.flags & assets::MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE != 0
            }),
        _ => false,
    };
    if !cube || block.support != VisualSupport::Exact || block.animation != NO_ANIMATION {
        return None;
    }
    let texture = overlay.texture.as_ref()?;
    // Overlay layers share the largest source's tile size; their 16-texel mip is the face.
    let mip = texture.mips.iter().find(|mip| mip.size as usize == TILE)?;
    let mut tiles = Vec::with_capacity(BlockFace::ALL.len());
    for id in block.faces {
        // Overlay materials address the overlay's own array as page 1.
        let material = overlay
            .materials
            .get(id as usize)
            .filter(|material| id != DIAGNOSTIC_MATERIAL && material.texture.page() == 1)?;
        tiles.push(face_tile(material, texture, mip)?);
    }
    compose_block_item_sheet(&tiles.try_into().ok()?)
}

/// Preserves face alpha; unresolved world tint and animation require authored carried faces.
fn face_tile(material: &Material, texture: &TextureArray, mip: &TextureMip) -> Option<IconSprite> {
    if material.flags
        & !(assets::MATERIAL_FLAG_ALPHA_BLEND
            | assets::MATERIAL_FLAG_ALPHA_CUTOUT
            | assets::MATERIAL_FLAG_ISOTROPIC)
        != 0
        || material.animation != NO_ANIMATION
        || mip.size as usize != TILE
        || material.texture.layer() >= texture.layers
    {
        return None;
    }
    let tile_bytes = TILE * TILE * 4;
    let first = (material.texture.layer() as usize).checked_mul(tile_bytes)?;
    let tile = mip.rgba8.get(first..first.checked_add(tile_bytes)?)?;
    Some(IconSprite {
        width: BLOCK_ITEM_FACE_SIDE,
        height: BLOCK_ITEM_FACE_SIDE,
        rgba8: tile.into(),
    })
}

/// The `[u0, v0, u1, v1]` region of each face's tile within a sheet placed at `region`.
pub fn face_rects(region: [f32; 4]) -> [[f32; 4]; 6] {
    let (width, height) = (region[2] - region[0], region[3] - region[1]);
    let columns = usize::from(BLOCK_ITEM_SHEET_GRID[0]);
    let [grid_width, grid_height] = BLOCK_ITEM_SHEET_GRID.map(f32::from);
    std::array::from_fn(|face| {
        let (column, row) = ((face % columns) as f32, (face / columns) as f32);
        [
            region[0] + width * column / grid_width,
            region[1] + height * row / grid_height,
            region[0] + width * (column + 1.0) / grid_width,
            region[1] + height * (row + 1.0) / grid_height,
        ]
    })
}
