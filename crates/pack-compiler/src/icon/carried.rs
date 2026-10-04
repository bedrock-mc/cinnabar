//! Carried opaque-mask texture baking. The source's alpha interpolates the
//! overlay tint; it is not the rendered face opacity.

use std::{path::Path, sync::Arc};

use assets::{AssetError, BLOCK_ITEM_FACE_SIDE, IconSprite};

use super::blocks::IconBlocks;

pub(super) fn tile(
    root: &Path,
    path: &str,
    overlay: Option<&str>,
) -> Result<Option<IconSprite>, AssetError> {
    let Some(sprite) = IconBlocks::sprite(root, path)?.filter(|sprite| {
        sprite.width == BLOCK_ITEM_FACE_SIDE && sprite.height == BLOCK_ITEM_FACE_SIDE
    }) else {
        return Ok(None);
    };
    let Some(overlay) = overlay else {
        return Ok(Some(sprite));
    };
    let Some(color) = overlay_rgb(overlay) else {
        return Ok(None);
    };
    let mut pixels = sprite.rgba8.to_vec();
    for pixel in pixels.chunks_exact_mut(4) {
        let mask = f32::from(pixel[3]) / 255.0;
        for (source, color) in pixel[..3].iter_mut().zip(color) {
            let source_rgb = f32::from(*source) / 255.0;
            let output = source_rgb * (1.0 - mask) + (f32::from(color) / 255.0) * source_rgb * mask;
            // The target atlas bake converts normalized output with a
            // truncating float-to-integer conversion, not nearest rounding.
            *source = (output * 255.0).clamp(0.0, 255.0) as u8;
        }
        pixel[3] = 255;
    }
    Ok(Some(IconSprite {
        rgba8: Arc::from(pixels),
        ..sprite
    }))
}

fn overlay_rgb(source: &str) -> Option<[u8; 3]> {
    // Matched 26.50 TextureJSONParser keeps the low RGB bytes
    // of the hexadecimal value and forces opacity to one. The matching
    // TextureAtlas::updateTextureAtUVs mixes original/tinted RGB
    // by sample alpha and forces positive-overlay alpha to one.
    // Malformed pack extensions remain unresolved instead of guessed.
    let hex = source.strip_prefix('#')?;
    if !matches!(hex.len(), 6 | 8) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let rgb = u32::from_str_radix(hex, 16).ok()?;
    Some([(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8])
}
