//! Native dropped-block cube textures shared by app and browser publication.
use crate::{DroppedItemCube, MAX_ITEM_SPRITE_SIDE, rope_color};
use assets::{
    BlockFace, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_TINT_MASK,
    MATERIAL_FLAG_WATER_TINT, NetworkIdMode, RuntimeAssets, VisualKind,
};
use std::sync::Arc;
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
    rope_color(r, g, b)
}

/// Builds a unit cube from a cube-kind block's six face textures, or `None` for other kinds.
pub fn block_cube(assets: &RuntimeAssets, mode: NetworkIdMode, id: u32) -> Option<DroppedItemCube> {
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
        if size == 0 || size > MAX_ITEM_SPRITE_SIDE || *tile_size.get_or_insert(size) != size {
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
