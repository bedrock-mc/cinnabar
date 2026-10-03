//! Plain opaque block cubes held in hand and drawn in slots: six 16-texel tiles composed into one
//! 48x32 sheet, for vanilla block items and for server custom block items alike.

use std::collections::BTreeMap;

use assets::{
    BLOCK_ITEM_FACE_SIDE, BLOCK_ITEM_SHEET_GRID, BlockFace, BlockFlags, BlockOverlay,
    DIAGNOSTIC_MATERIAL, IconSprite, ItemVisualDefinitionRoute, Material, NO_ANIMATION,
    NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets, RuntimeEntityAssets, TextureArray, TextureMip,
    VisualKind, VisualSupport, compose_block_item_sheet,
};

pub(super) const TILE: usize = BLOCK_ITEM_FACE_SIDE as usize;

pub(crate) struct BlockSheets {
    pub(crate) sheets: Vec<IconSprite>,
    /// Block visual id to its sheet index.
    pub(crate) by_visual: BTreeMap<u32, usize>,
}

/// A sheet per distinct set of face tiles, for every block item whose block is an ordinary
/// opaque cube; other blocks are skipped.
pub(crate) fn collect(world: &RuntimeAssets, entities: &RuntimeEntityAssets) -> BlockSheets {
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

fn cube_materials(world: &RuntimeAssets, visual: u32) -> Option<[u32; 6]> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    if !block.is_known()
        || block.kind() != VisualKind::Cube
        || block.support() != VisualSupport::Exact
        || block.flags() != (BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        || block.model_template().is_some()
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
            page.texture.mips.first()?,
        )?);
    }
    compose_block_item_sheet(&tiles.try_into().ok()?)
}

/// The sheet of a session overlay's state `visual` when it is a plain opaque cube, held and
/// drawn in slots like a vanilla block item's (`collect`); `None` for any other shape.
pub(crate) fn overlay_sheet(overlay: &BlockOverlay, visual: usize) -> Option<IconSprite> {
    let block = overlay.visuals.get(visual)?;
    if block.kind != VisualKind::Cube
        || block.flags != (BlockFlags::CUBE_GEOMETRY | BlockFlags::OCCLUDES_FULL_FACE)
        || block.model_template != NO_MODEL_TEMPLATE
        || block.animation != NO_ANIMATION
    {
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

/// `material`'s 16-texel layer of `mip`; tinted, alpha-flagged or animated materials have none.
fn face_tile(material: &Material, texture: &TextureArray, mip: &TextureMip) -> Option<IconSprite> {
    if material.flags != 0
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
pub(super) fn face_rects(region: [f32; 4]) -> [[f32; 4]; 6] {
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
