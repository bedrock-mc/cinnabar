//! Inventory-only shapes whose materials differ from their placed fallback.

use super::{blocks::IconBlocks, model::Model};
use assets::{AssetError, BlockVisualId, IconSprite};
use std::path::Path;

/// Selects an explicit carried shape before generic cube and placed-model baking.
pub(super) fn has_model(name: &str) -> bool {
    name == assets::facts::pack::PackBlock::DriedGhast.identifier()
}

/// Bakes a carried dried ghast, refusing incomplete body or tentacle artwork.
pub(super) fn raster(
    root: &Path,
    blocks: &IconBlocks,
    visual: BlockVisualId,
) -> Result<Option<IconSprite>, AssetError> {
    let Some(body) = blocks.terrain_tiles(root, visual)? else {
        return Ok(None);
    };
    let Some(path) = blocks.texture_path(visual) else {
        return Ok(None);
    };
    let Some(tentacles) = IconBlocks::model_texture(root, &path)? else {
        return Ok(None);
    };
    let textures: Vec<_> = body.iter().chain(std::iter::once(&tentacles)).collect();
    let quads = assets::gui_item::dried_ghast_quads()
        .into_iter()
        .map(|quad| (quad.corners, quad.uvs, textures[quad.material as usize]));
    Ok(Some(
        Model::textured(quads, assets::gui_item::project_dried_ghast, 1.0).raster(),
    ))
}
