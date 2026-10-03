use assets::{
    MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_DRY_FOLIAGE, MATERIAL_FLAG_EVERGREEN_FOLIAGE,
    MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT, MATERIAL_FLAG_GRASS_TINT,
    MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_WATER_TINT, NetworkIdMode, RuntimeAssets,
    RuntimeIconCatalog,
};
use client_world::WorldStream;
use render::TileRequest;

#[cfg(test)]
use assets::BlockFace;
#[cfg(test)]
mod tests;

/// A block-textured particle tile with its gamma-space biome tint.
pub(super) struct BlockTile {
    pub(super) tile: TileRequest,
    pub(super) tint: [f32; 4],
}

/// Converts the world tint into the gamma-space colour expected by particle Molang.
fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Uses the resolved down texture and the block's biome tint, as vanilla terrain effects do.
pub(super) fn block_tile(
    stream: &WorldStream,
    mode: NetworkIdMode,
    network_id: u32,
    block: [i32; 3],
) -> Option<BlockTile> {
    let (tile, flags) = resolved_tile(stream.runtime_assets(), mode, network_id)?;
    Some(BlockTile {
        tile,
        tint: biome_tint(stream, flags, block),
    })
}

fn resolved_tile(
    assets: &RuntimeAssets,
    mode: NetworkIdMode,
    network_id: u32,
) -> Option<(TileRequest, u32)> {
    render::block_particle_tile(assets, mode, network_id)
}

/// Gamma-space biome colour for a material's tint mode; white when untinted.
fn biome_tint(stream: &WorldStream, flags: u32, block: [i32; 3]) -> [f32; 4] {
    let mode = flags & MATERIAL_FLAG_TINT_MASK;
    if mode == 0 {
        return [1.0; 4];
    }
    let center = block.map(|c| c as f32 + 0.5);
    let Some(raw) = stream.camera_biome_id(center) else {
        return [1.0; 4];
    };
    let tints = stream.resolved_biome_tints_snapshot();
    let Some(record) = tints.records.get(tints.dense_index(raw) as usize) else {
        return [1.0; 4];
    };
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
    [
        linear_to_srgb(linear[0]),
        linear_to_srgb(linear[1]),
        linear_to_srgb(linear[2]),
        1.0,
    ]
}

pub(super) fn item_tile(
    icons: &RuntimeIconCatalog,
    identifier: &str,
    metadata: u32,
) -> Option<TileRequest> {
    render::item_particle_tile(icons, identifier, metadata)
}
