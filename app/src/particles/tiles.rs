use assets::{MATERIAL_FLAG_TINT_MASK, NetworkIdMode};
use chunk_pipeline::WorldStream;
use particles::{TileRequest, tiles::material_tint};

/// A block-textured particle tile with its gamma-space biome tint.
pub(super) struct BlockTile {
    pub(super) tile: TileRequest,
    pub(super) tint: [f32; 4],
}

pub(super) fn block_tile(
    stream: &WorldStream,
    mode: NetworkIdMode,
    network_id: u32,
    block: [i32; 3],
) -> Option<BlockTile> {
    let (tile, flags) = particles::tiles::terrain_tile(stream.runtime_assets(), mode, network_id)?;
    Some(BlockTile {
        tile,
        tint: biome_tint(stream, flags, block),
    })
}

/// Gamma-space biome colour at `block` for a material's tint mode; white when untinted.
pub(super) fn biome_tint(stream: &WorldStream, flags: u32, block: [i32; 3]) -> [f32; 4] {
    if flags & MATERIAL_FLAG_TINT_MASK == 0 {
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
    material_tint(record, flags)
}

#[cfg(test)]
mod tests {
    use assets::{BlockFace, MATERIAL_FLAG_TINT_MASK, NetworkIdMode, RuntimeAssets, TextureRef};
    use particles::tiles::terrain_tile;

    #[test]
    #[ignore = "requires the local compiled vanilla world carrier"]
    fn real_carrier_terrain_tiles_resolve_stone_deepslate_and_grass() {
        let bytes = std::fs::read(crate::asset_startup::DEFAULT_ASSET_PATH).unwrap();
        let assets = RuntimeAssets::decode(&bytes).unwrap();
        let records = assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap();
        for name in [
            "minecraft:stone",
            "minecraft:deepslate",
            "minecraft:grass_block",
        ] {
            let states = records
                .iter()
                .filter(|record| &*record.name == name)
                .collect::<Vec<_>>();
            assert!(!states.is_empty(), "pinned registry is missing {name}");
            for record in states {
                let (tile, flags) =
                    terrain_tile(&assets, NetworkIdMode::Sequential, record.sequential_id).unwrap();
                let down = assets.material(
                    assets
                        .resolve(NetworkIdMode::Sequential, record.sequential_id)
                        .face(BlockFace::Down)
                        .material_id(),
                );
                assert_eq!(
                    tile.key,
                    (u64::from(down.texture.page()) << 32) | u64::from(down.texture.layer())
                );
                assert_ne!(down.texture, TextureRef::DIAGNOSTIC);
                assert!(!tile.pixels.as_chunks::<4>().0.contains(&[255, 0, 255, 255]));
                if name == "minecraft:grass_block" {
                    assert_eq!(flags & MATERIAL_FLAG_TINT_MASK, 0);
                }
            }
        }
    }
}
