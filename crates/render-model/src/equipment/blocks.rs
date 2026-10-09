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
    /// Exact carried models with sheet-relative UVs, in centred block coordinates.
    pub models: BTreeMap<u32, Vec<crate::ActorRigVertex>>,
}

/// Shares face sheets across cube items independently of their terrain occlusion and alpha.
pub fn collect(world: &RuntimeAssets, entities: &RuntimeEntityAssets) -> BlockSheets {
    let mut sheets = Vec::new();
    let mut by_materials = BTreeMap::<[u32; 6], usize>::new();
    let mut by_visual = BTreeMap::new();
    let mut models = BTreeMap::new();
    if !world.provenance().is_complete() {
        return BlockSheets {
            sheets,
            by_visual,
            models,
        };
    }
    for definition in entities.item_visuals() {
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            continue;
        };
        let visual = block_visual.0;
        if by_visual.contains_key(&visual) || visual as usize >= world.visual_count() {
            continue;
        }
        let model = (definition.key.identifier.as_ref() == assets::END_PORTAL_FRAME_IDENTIFIER)
            .then(|| frame_model(world, visual))
            .flatten();
        let Some(materials) = model
            .as_ref()
            .map(|(materials, _)| *materials)
            .or_else(|| cube_materials(world, visual))
        else {
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
        if let Some((_, vertices)) = model {
            models.insert(visual, vertices);
        }
    }
    BlockSheets {
        sheets,
        by_visual,
        models,
    }
}

/// Reuses the exact unfilled portal frame template instead of inventing a full cube.
fn frame_model(
    world: &RuntimeAssets,
    visual: u32,
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    if block.kind() != VisualKind::Model || block.support() != VisualSupport::Exact {
        return None;
    }
    let template = world
        .model_templates()
        .get(block.model_template()? as usize)?;
    if template.flags != 0 || template.quad_count != BlockFace::ALL.len() as u32 {
        return None;
    }
    let quads = world.model_quads().get(
        template.quad_start as usize..template.quad_start as usize + template.quad_count as usize,
    )?;
    let rects = face_rects([0.0, 0.0, 1.0, 1.0]);
    let mut materials = [0; 6];
    let mut vertices = Vec::with_capacity(36);
    for (index, quad) in quads.iter().enumerate() {
        let face = index;
        if quad.flags & assets::MODEL_QUAD_FLAG_FACE_MASK
            != BlockFace::ALL[face].model_quad_face_id()
        {
            return None;
        }
        materials[face] = quad.material;
        let rect = rects[face];
        let normal = match BlockFace::ALL[face] {
            BlockFace::West => [-1.0, 0.0, 0.0],
            BlockFace::East => [1.0, 0.0, 0.0],
            BlockFace::Down => [0.0, -1.0, 0.0],
            BlockFace::Up => [0.0, 1.0, 0.0],
            BlockFace::North => [0.0, 0.0, -1.0],
            BlockFace::South => [0.0, 0.0, 1.0],
        };
        for corner in [0, 1, 2, 0, 2, 3] {
            let uv = std::array::from_fn(|axis| {
                rect[axis]
                    + f32::from(quad.uvs[corner][axis]) / 4096.0 * (rect[axis + 2] - rect[axis])
            });
            vertices.push(crate::ActorRigVertex {
                position: quad.positions[corner]
                    .map(|coordinate| f32::from(coordinate) / 256.0 - 0.5),
                normal,
                uv,
                back_uv: uv,
                bone_index: 0,
                surface: crate::ActorRigSurface::SINGLE_FACE,
            });
        }
    }
    Some((materials, vertices))
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
    let mut tiles = Vec::with_capacity(BlockFace::ALL.len());
    for id in block.faces {
        // Overlay materials address the overlay's own array as page 1.
        let material = overlay
            .materials
            .get(id as usize)
            .filter(|material| id != DIAGNOSTIC_MATERIAL && material.texture.page() == 1)?;
        tiles.push(overlay_face_tile(overlay, material)?);
    }
    compose_block_item_sheet(&tiles.try_into().ok()?)
}

/// Selects mips in exposed source pixels and resamples the admitted grid into a held face.
fn overlay_face_tile(overlay: &BlockOverlay, material: &Material) -> Option<IconSprite> {
    if !face_material_is_admitted(material) {
        return None;
    }
    let texture = overlay.texture.as_ref()?;
    let layer = material.texture.layer() as usize;
    if layer >= texture.layers as usize {
        return None;
    }
    let base = texture.mips.first()?;
    let grid = overlay
        .texture_source_grids
        .get(layer)
        .copied()
        .unwrap_or(0);
    let source_side = match overlay.texture_source_sizes.get(layer) {
        Some(size) => u32::from(*size.iter().max()?),
        None => base.size.checked_shr(u32::from(grid))?,
    };
    if source_side == 0 {
        return None;
    }
    let level = source_side
        .ilog2()
        .saturating_sub(TILE.ilog2())
        .min(assets::VANILLA_TERRAIN_MIP_COUNT - 1);
    let mip = texture.mips.get(level as usize)?;
    if mip.size == 0 || mip.size > assets::MAX_TILE_SIZE {
        return None;
    }
    let side = mip.size as usize;
    let exposed = side.checked_shr(u32::from(grid))?;
    if exposed == 0 {
        return None;
    }
    let bytes = side.checked_mul(side)?.checked_mul(4)?;
    let start = layer.checked_mul(bytes)?;
    let tile = mip.rgba8.get(start..start.checked_add(bytes)?)?;
    let mut pixels = Vec::with_capacity(TILE * TILE * 4);
    for y in 0..TILE {
        for x in 0..TILE {
            let offset = ((y * exposed / TILE) * side + x * exposed / TILE) * 4;
            pixels.extend_from_slice(&tile[offset..offset + 4]);
        }
    }
    Some(IconSprite {
        width: BLOCK_ITEM_FACE_SIDE,
        height: BLOCK_ITEM_FACE_SIDE,
        rgba8: pixels.into(),
    })
}

/// Unresolved tint and animation need authored carried faces instead of a static sheet.
fn face_material_is_admitted(material: &Material) -> bool {
    material.flags
        & !(assets::MATERIAL_FLAG_ALPHA_BLEND
            | assets::MATERIAL_FLAG_ALPHA_CUTOUT
            | assets::MATERIAL_FLAG_ISOTROPIC)
        == 0
        && material.animation == NO_ANIMATION
}

/// Preserves face alpha; unresolved world tint and animation require authored carried faces.
fn face_tile(material: &Material, texture: &TextureArray, mip: &TextureMip) -> Option<IconSprite> {
    if !face_material_is_admitted(material)
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
