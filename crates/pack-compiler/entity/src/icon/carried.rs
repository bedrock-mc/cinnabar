//! Carried opaque-mask texture baking. The source's alpha interpolates the
//! overlay tint; it is not the rendered face opacity.

use std::{path::Path, sync::Arc};

use assets::{AssetError, BLOCK_ITEM_FACE_SIDE, IconSprite};

use super::blocks::IconBlocks;

/// Bakes a native UV flip into a carried sheet's square face without resampling its texels.
pub(super) fn flip(mut sprite: IconSprite, flip: u8) -> Result<IconSprite, AssetError> {
    if flip == 0 {
        return Ok(sprite);
    }
    let source = image::RgbaImage::from_raw(
        u32::from(sprite.width),
        u32::from(sprite.height),
        sprite.rgba8.to_vec(),
    )
    .ok_or_else(|| AssetError::InvalidCompiledAssets {
        detail: "invalid carried face pixels".into(),
    })?;
    let rotated = match flip {
        1 => image::imageops::rotate90(&source),
        2 => image::imageops::rotate270(&source),
        3 => image::imageops::rotate180(&source),
        _ => {
            return Err(AssetError::InvalidCompiledAssets {
                detail: "invalid carried face UV flip".into(),
            });
        }
    };
    sprite.rgba8 = Arc::from(rotated.into_raw());
    Ok(sprite)
}

#[cfg(test)]
mod flip_tests {
    use super::*;

    #[test]
    fn carried_face_uv_flips_preserve_corner_artwork() {
        let pixels: Vec<_> = [1, 2, 3, 4]
            .into_iter()
            .flat_map(|red| [red, 0, 0, 255])
            .collect();
        let sprite = IconSprite {
            width: 2,
            height: 2,
            rgba8: pixels.into(),
        };
        for (value, expected) in [
            (0, [1, 2, 3, 4]),
            (1, [3, 1, 4, 2]),
            (2, [2, 4, 1, 3]),
            (3, [4, 3, 2, 1]),
        ] {
            let shown = flip(sprite.clone(), value).unwrap();
            assert_eq!(
                shown
                    .rgba8
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|pixel| pixel[0])
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}

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
    if let Some(overlay) = overlay {
        let Some(color) = overlay_rgb(overlay) else {
            return Ok(None);
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
