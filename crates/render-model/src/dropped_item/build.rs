//! CPU builders for dropped blocks from retained world geometry and texture pages.
use std::{collections::HashMap, sync::Arc};

use assets::{
    BlockFace, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_TINT_MASK,
    MATERIAL_FLAG_WATER_TINT, NetworkIdMode, RuntimeAssets, VisualKind,
};

use super::{DroppedItemBlock, DroppedItemCube, DroppedItemSprite};

const GRASS_TINT_RGB: [u8; 3] = [0x79, 0xc0, 0x5a];
const FOLIAGE_TINT_RGB: [u8; 3] = [0x77, 0xab, 0x2f];
const WATER_TINT_RGB: [u8; 3] = [0x3f, 0x76, 0xe4];

fn tint_rgba(flags: u32) -> u32 {
    let [r, g, b] = match flags & MATERIAL_FLAG_TINT_MASK {
        MATERIAL_FLAG_GRASS_TINT => GRASS_TINT_RGB,
        MATERIAL_FLAG_FOLIAGE_TINT => FOLIAGE_TINT_RGB,
        MATERIAL_FLAG_WATER_TINT => WATER_TINT_RGB,
        _ => [255; 3],
    };
    u32::from_le_bytes([r, g, b, 255])
}

/// Builds a unit cube from a cube-kind block's six face textures, or `None` for other kinds.
pub fn dropped_item_block_cube(
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    id: u32,
    max_tile_side: u32,
) -> Option<DroppedItemCube> {
    let block = assets.resolve(mode, id);
    if !block.is_known() || block.kind() != VisualKind::Cube {
        return None;
    }
    let mut tile_size = None;
    let mut faces: Vec<Arc<[u8]>> = Vec::with_capacity(6);
    let mut tints = [0_u32; 6];
    for (index, face) in BlockFace::ALL.into_iter().enumerate() {
        let material = assets.material(block.face(face).material_id());
        let page = assets
            .texture_pages()
            .get(material.texture.page() as usize)?;
        let mip = page.texture.mips.first()?;
        let size = mip.size;
        if size == 0 || size > max_tile_side || *tile_size.get_or_insert(size) != size {
            return None;
        }
        let bytes = (size * size * 4) as usize;
        let start = material.texture.layer() as usize * bytes;
        faces.push(Arc::from(mip.rgba8.get(start..start + bytes)?));
        tints[index] = tint_rgba(material.flags);
    }
    Some(DroppedItemCube {
        tile: tile_size?,
        faces: faces.try_into().ok()?,
        tints,
    })
}

/// Retains authored block model quads and material tiles, or `None` for other kinds.
pub fn dropped_item_block_model(
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    id: u32,
    max_tile_side: u32,
) -> Option<DroppedItemBlock> {
    let block = assets.resolve(mode, id);
    if !block.is_known() || !matches!(block.kind(), VisualKind::Model | VisualKind::Cross) {
        return None;
    }
    let mut template_id = block.model_template()?;
    let mut quads = Vec::new();
    let mut materials = Vec::new();
    let mut material_indices = HashMap::new();
    loop {
        let template = assets.model_templates().get(template_id as usize)?;
        let first = template.quad_start as usize;
        for quad in assets
            .model_quads()
            .get(first..first + template.quad_count as usize)?
        {
            let mut quad = *quad;
            let material_index = if let Some(index) = material_indices.get(&quad.material) {
                *index
            } else {
                let material = assets.material(quad.material);
                let page = assets
                    .texture_pages()
                    .get(material.texture.page() as usize)?;
                let mip = page.texture.mips.first()?;
                let size = mip.size;
                if size == 0 || size > max_tile_side {
                    return None;
                }
                let bytes = (size * size * 4) as usize;
                let first = material.texture.layer() as usize * bytes;
                let index = materials.len() as u32;
                materials.push((
                    DroppedItemSprite {
                        width: size,
                        height: size,
                        rgba8: Arc::from(mip.rgba8.get(first..first + bytes)?),
                    },
                    tint_rgba(material.flags),
                ));
                material_indices.insert(quad.material, index);
                index
            };
            quad.material = material_index;
            quads.push(quad);
        }
        if template.flags & assets::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            break;
        }
        template_id = template_id.checked_add(1)?;
    }
    Some(DroppedItemBlock {
        materials: materials.into(),
        quads: quads.into(),
        rotation: block.variant() & 3,
    })
}
