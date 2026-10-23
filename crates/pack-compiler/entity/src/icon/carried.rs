//! Carried opaque-mask texture baking. The source's alpha interpolates the
//! overlay tint; it is not the rendered face opacity.

use std::{path::Path, sync::Arc};

use assets::{AssetError, BLOCK_ITEM_FACE_SIDE, IconSprite};

use super::blocks::IconBlocks;

/// Bakes a carried face, optionally selecting a strip's first frame before applying its mask.
pub(super) fn tile(
    root: &Path,
    path: &str,
    overlay: Option<&str>,
    first_strip_frame: bool,
) -> Result<Option<IconSprite>, AssetError> {
    let Some(mut sprite) = IconBlocks::sprite_with_strip_frame(root, path, first_strip_frame)?
        .filter(|sprite| {
            if first_strip_frame {
                sprite.width == sprite.height && sprite.width >= BLOCK_ITEM_FACE_SIDE
            } else {
                sprite.width == BLOCK_ITEM_FACE_SIDE && sprite.height == BLOCK_ITEM_FACE_SIDE
            }
        })
    else {
        return Ok(None);
    };
    if apply_overlay(&mut sprite, overlay).is_none() {
        return Ok(None);
    }
    if first_strip_frame && sprite.width > BLOCK_ITEM_FACE_SIDE {
        // Overlay alpha is a tint weight; resolve it before opacity-aware resampling.
        sprite.rgba8 = Arc::from(pack_compile_input::image::normalize_texture_tile(
            sprite.rgba8.to_vec().into_boxed_slice(),
            u32::from(sprite.width),
            path,
        )?);
        sprite.width = BLOCK_ITEM_FACE_SIDE;
        sprite.height = BLOCK_ITEM_FACE_SIDE;
    }
    Ok(Some(sprite))
}

/// Loads square model artwork without the fixed carried-sheet tile dimensions.
pub(super) fn model_tile(
    blocks: &IconBlocks,
    root: &Path,
    path: &str,
    overlay: Option<&str>,
) -> Result<Option<IconSprite>, AssetError> {
    let Some(mut sprite) = blocks
        .model_frame_sprite(root, path)?
        .filter(|sprite| sprite.width == sprite.height)
    else {
        return Ok(None);
    };
    if apply_overlay(&mut sprite, overlay).is_none() {
        return Ok(None);
    }
    Ok(Some(sprite))
}

/// Applies a carried color mask while retaining the source resolution.
fn apply_overlay(sprite: &mut IconSprite, overlay: Option<&str>) -> Option<()> {
    if let Some(overlay) = overlay {
        let Some(color) = overlay_rgb(overlay) else {
            return None;
        };
        let mut pixels = sprite.rgba8.to_vec();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let mask = f32::from(pixel[3]) / 255.0;
            for (source, color) in pixel[..3].iter_mut().zip(color) {
                let source_rgb = f32::from(*source) / 255.0;
                let output =
                    source_rgb * (1.0 - mask) + (f32::from(color) / 255.0) * source_rgb * mask;
                // The target atlas bake converts normalized output with a
                // truncating float-to-integer conversion, not nearest rounding.
                *source = (output * 255.0).clamp(0.0, 255.0) as u8;
            }
            pixel[3] = 255;
        }
        sprite.rgba8 = Arc::from(pixels);
    }
    Some(())
}

fn overlay_rgb(source: &str) -> Option<[u8; 3]> {
    // 26.50's texture JSON parsing keeps the low RGB bytes
    // of the hexadecimal value and forces opacity to one. Vanilla atlas
    // tinting mixes original/tinted RGB
    // by sample alpha and forces positive-overlay alpha to one.
    // Malformed pack extensions remain unresolved instead of guessed.
    let hex = source.strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8])
}
