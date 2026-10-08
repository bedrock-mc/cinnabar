//! Block and item textures as particle tiles, and their gamma-space tints.

use std::sync::Arc;

use assets::{
    BlockFace, LinearBiomeTints, MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_DRY_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT,
    MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_WATER_TINT, NetworkIdMode,
    RuntimeAssets, RuntimeIconCatalog, seasonal_foliage_palette_index,
};

use crate::emitter::TileRequest;

#[cfg(test)]
mod bottom_material_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tint_tests;

/// Marks item-icon tile keys so they never collide with block layer keys.
const ITEM_KEY_FLAG: u64 = 1 << 63;

/// Converts the world tint into the gamma-space colour expected by particle Molang.
#[must_use]
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// The block's terrain tile and its material flags; `None` for unknown or diagnostic blocks.
///
/// Native terrain particles use the bottom material, not a tint-based top/side heuristic.
/// Vanilla destruction particles resolve `down`, then `*`; the built-in texture fallback uses
/// texture group zero, populated from `down`.
#[must_use]
pub fn terrain_tile(
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
    let material = terrain_material(assets, mode, network_id);
    let flags = material.flags;
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

fn terrain_material(
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    network_id: u32,
) -> assets::Material {
    assets.material(
        assets
            .resolve(mode, network_id)
            .face(BlockFace::Down)
            .material_id(),
    )
}

/// Gamma-space biome colour for a material's tint mode; white when untinted.
#[must_use]
pub fn material_tint(record: &LinearBiomeTints, flags: u32) -> [f32; 4] {
    let mode = flags & MATERIAL_FLAG_TINT_MASK;
    let linear = if mode == MATERIAL_FLAG_WATER_TINT {
        record.water
    } else if mode == MATERIAL_FLAG_GRASS_TINT {
        record.grass
    } else if mode == MATERIAL_FLAG_FOLIAGE_TINT {
        match flags & MATERIAL_FLAG_FOLIAGE_CLASS_MASK {
            MATERIAL_FLAG_BIRCH_FOLIAGE => record.birch,
            MATERIAL_FLAG_EVERGREEN_FOLIAGE => record.evergreen,
            MATERIAL_FLAG_DRY_FOLIAGE => record.dry_foliage,
            _ => record.foliage,
        }
    } else {
        return [1.0; 4];
    };
    gamma(linear)
}

/// Gamma-space seasonal leaf colour for the covered or exposed palette column.
#[must_use]
pub fn seasonal_tint(record: &LinearBiomeTints, flags: u32, exposed: bool) -> [f32; 4] {
    gamma(record.seasonal_foliage[seasonal_foliage_palette_index(flags, exposed)])
}

fn gamma(linear: [f32; 4]) -> [f32; 4] {
    [
        linear_to_srgb(linear[0]),
        linear_to_srgb(linear[1]),
        linear_to_srgb(linear[2]),
        1.0,
    ]
}

/// The native height map bounds the upward scan after the last non-air block.
/// Scanning to the bounded dimension roof is equivalent for resident air, and
/// conservatively covers missing cells rather than guessing an unknown height map.
pub fn column_exposed(
    start: i32,
    minimum: i32,
    maximum: i32,
    mut shelters: impl FnMut(i32) -> bool,
) -> bool {
    start >= minimum && start < maximum && !(start..maximum).any(&mut shelters)
}

/// An item icon as a particle tile, keyed by its catalog sprite.
#[must_use]
pub fn item_tile(
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
