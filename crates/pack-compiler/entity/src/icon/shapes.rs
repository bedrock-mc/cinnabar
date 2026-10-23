//! Inventory-only shapes whose materials differ from their placed fallback.

use super::{
    blocks::IconBlocks,
    model::{Model, TexturedQuad},
};
use assets::facts::pack::PackBlock as B;
use assets::{AssetError, BlockFace, BlockVisualId, IconSprite};
use std::{path::Path, sync::Arc};

/// Selects an explicit carried shape before generic cube and placed-model baking.
pub(super) fn has_model(name: &str) -> bool {
    [B::DriedGhast, B::SculkSensor, B::CalibratedSculkSensor]
        .iter()
        .any(|kind| name == kind.identifier())
}

/// Bakes carried geometry while keeping the body and attachment textures separate.
pub(super) fn raster(
    root: &Path,
    blocks: &IconBlocks,
    visual: BlockVisualId,
) -> Result<Option<IconSprite>, AssetError> {
    let Some(body) = blocks.terrain_tiles(root, visual)? else {
        return Ok(None);
    };
    let dried = blocks.name(visual) == Some(B::DriedGhast.identifier());
    let calibrated = blocks.name(visual) == Some(B::CalibratedSculkSensor.identifier());
    let Some(path) = blocks.attachment_path(visual, BlockFace::Down) else {
        return Ok(None);
    };
    let Some(mut attachment) = IconBlocks::model_texture(root, &path)? else {
        return Ok(None);
    };
    if !dried {
        for pixel in Arc::make_mut(&mut attachment.rgba8).chunks_exact_mut(4) {
            for channel in &mut pixel[..3] {
                *channel = (u16::from(*channel)
                    * u16::from(assets::gui_item::SENSOR_TENDRIL_BRIGHTNESS)
                    / 255) as u8;
            }
        }
    }
    let mut textures = body.to_vec();
    textures.push(attachment);
    if calibrated {
        let Some(path) = blocks.attachment_path(visual, BlockFace::South) else {
            return Ok(None);
        };
        let Some(crystal) = IconBlocks::model_texture(root, &path)? else {
            return Ok(None);
        };
        textures.push(crystal);
    }
    let quads = if dried {
        assets::gui_item::dried_ghast_quads()
            .into_iter()
            .map(|quad| (quad, true))
            .collect()
    } else {
        assets::gui_item::sculk_sensor_quads(calibrated)
    };
    let quads = quads.into_iter().map(|(quad, face_dimming)| TexturedQuad {
        corners: quad.corners,
        uvs: quad.uvs,
        texture: &textures[quad.material as usize],
        face_dimming,
    });
    let projection = if dried {
        assets::gui_item::project_dried_ghast
    } else {
        assets::gui_item::project_cube
    };
    Ok(Some(Model::textured(quads, projection, 1.0).raster()))
}
