//! Shared canonical inventory sprite selection for particle tile effects.
use super::TileRequest;
use assets::{
    BlockFace, MATERIAL_FLAG_TINT_MASK, NetworkIdMode, RuntimeAssets, RuntimeIconCatalog,
};
use std::sync::Arc;
const ITEM_KEY_FLAG: u64 = 1 << 63;

/// An item icon as a particle tile, keyed by its catalog sprite.
pub fn item_particle_tile(
    icons: &RuntimeIconCatalog,
    identifier: &str,
    metadata: u32,
) -> Option<TileRequest> {
    let index = icons.lookup_index(identifier, metadata)?;
    let sprite = icons.sprites().get(index)?;
    // The particle tile is square; a non-square sprite is skipped rather than distorted.
    if sprite.width != sprite.height || sprite.width == 0 {
        return None;
    }
    Some(TileRequest {
        key: ITEM_KEY_FLAG | index as u64,
        size: u32::from(sprite.width),
        pixels: Arc::clone(&sprite.rgba8),
    })
}

/// Copies the selected state texture; tint policy is independent of that texture's face.
pub fn block_particle_tile(
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    network_id: u32,
) -> Option<(TileRequest, u32)> {
    if !assets.is_known(mode, network_id) {
        return None;
    }
    let resolved = assets.resolve(mode, network_id);
    let id = resolved.face(BlockFace::Down).material_id();
    if id == assets::DIAGNOSTIC_MATERIAL {
        return None;
    }
    let material = assets.material(id);
    let flags = [BlockFace::Up, BlockFace::North, BlockFace::Down]
        .into_iter()
        .map(|face| assets.material(resolved.face(face).material_id()).flags)
        .find(|flags| flags & MATERIAL_FLAG_TINT_MASK != 0)
        .unwrap_or(0);
    let page = assets
        .texture_pages()
        .get(material.texture.page() as usize)?;
    let mip = page.texture.mips.first()?;
    let layer = material.texture.layer();
    if layer >= page.texture.layers {
        return None;
    }
    let stride = (mip.size * mip.size * 4) as usize;
    let start = layer as usize * stride;
    let pixels = mip.rgba8.get(start..start + stride)?;
    Some((
        TileRequest {
            key: (u64::from(material.texture.page()) << 32) | u64::from(layer),
            size: mip.size,
            pixels: Arc::from(pixels),
        },
        flags,
    ))
}
