mod block;
mod fixed_tint;
mod flipbook;
mod model_selectors;
mod parse;
mod terrain;
mod variations;
pub use fixed_tint::{apply_atlas_tint, parse_atlas_tint};

use std::path::Path;

use assets::{AssetError, texture::flipbook::FlipbookSource};
pub use block::{BlockTextureMap, TextureKey, resolve_texture_key};
pub use block::{
    has_runtime_cube_face_mapping, resolve_carried_down_key, resolve_carried_face_key,
    resolve_carried_fallback_key, runtime_binding_identity,
};
pub use flipbook::MAX_FLIPBOOKS;
pub use terrain::TerrainTextureMap;

use block::read_blocks;
use flipbook::read_flipbooks;
pub use terrain::read_terrain;

/// Parsed vanilla pack sources used by the later deterministic compiler.
#[derive(Debug)]
pub struct PackSources {
    pub blocks: BlockTextureMap,
    pub terrain: TerrainTextureMap,
    pub flipbooks: Box<[FlipbookSource]>,
}

/// Reads the bounded JSON source subset needed by the vanilla texture compiler.
pub fn read_pack(root: &Path) -> Result<PackSources, AssetError> {
    let blocks_path = root.join("blocks.json");
    let terrain_path = root.join("textures/terrain_texture.json");
    let flipbooks_path = root.join("textures/flipbook_textures.json");

    let blocks = read_blocks(&blocks_path)?;
    let terrain = read_terrain(&terrain_path)?;
    validate_block_keys(&blocks, &terrain)?;
    let flipbooks = read_flipbooks(&flipbooks_path, &terrain)?;

    Ok(PackSources {
        blocks,
        terrain,
        flipbooks,
    })
}

fn validate_block_keys(
    blocks: &BlockTextureMap,
    terrain: &TerrainTextureMap,
) -> Result<(), AssetError> {
    for (block, entry) in &blocks.entries {
        entry.textures.try_for_each_key(|key| {
            if terrain.entries.contains_key(key) {
                Ok(())
            } else {
                Err(AssetError::MissingTerrainKey {
                    block: block.clone(),
                    key: key.into(),
                })
            }
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod terrain_selector_tests;
