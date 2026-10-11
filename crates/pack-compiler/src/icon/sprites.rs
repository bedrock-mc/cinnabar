//! Source-deduplicated flat icon baking and bounded texture decoding.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use assets::{AssetError, CompiledEntityAssets, IconSprite, MAX_ICON_SIDE};
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(super) struct SpriteBaker {
    by_source: BTreeMap<u32, Option<u32>>,
    pub(super) animation_strips: usize,
    pub(super) skipped_oversized: usize,
}

impl SpriteBaker {
    /// Decodes each source once and retains its sprite index or bounded refusal.
    pub(super) fn bake(
        &mut self,
        root: &Path,
        compiled: &CompiledEntityAssets,
        source_index: u32,
        sprites: &mut Vec<IconSprite>,
    ) -> Result<Option<u32>, AssetError> {
        if let Some(existing) = self.by_source.get(&source_index) {
            return Ok(*existing);
        }
        let source = &compiled.sources[source_index as usize];
        let mut decoded = sprite_source(root, source)?;
        if source
            .path
            .strip_prefix("textures/items/leather_")
            .is_some_and(|suffix| {
                let stem = suffix
                    .strip_suffix(".png")
                    .or_else(|| suffix.strip_suffix(".tga"));
                matches!(
                    stem,
                    Some("helmet" | "chestplate" | "leggings" | "boots" | "horse_armor")
                )
            })
        {
            let dye = assets::DEFAULT_LEATHER_RGB;
            let tint = [(dye >> 16) as u8, (dye >> 8) as u8, dye as u8];
            for pixel in decoded.rgba8.as_chunks_mut::<4>().0 {
                let texel = [pixel[0], pixel[1], pixel[2], pixel[3]];
                pixel.copy_from_slice(&assets::color_mask_texel(texel, tint));
            }
        }
        let Some((sprite, strip)) = bounded_sprite(decoded) else {
            self.skipped_oversized += 1;
            self.by_source.insert(source_index, None);
            return Ok(None);
        };
        self.animation_strips += usize::from(strip);
        let index =
            u32::try_from(sprites.len()).map_err(|_| AssetError::InvalidCompiledAssets {
                detail: "icon sprite count exceeds platform".into(),
            })?;
        sprites.push(sprite);
        self.by_source.insert(source_index, Some(index));
        Ok(Some(index))
    }
}

/// Bounds a decoded texture to a flat icon: as-is within `MAX_ICON_SIDE`, else a vertical
/// animation strip's first frame (`true`); anything else is refused.
pub(super) fn bounded_sprite(decoded: crate::image::DecodedTexture) -> Option<(IconSprite, bool)> {
    let (width, height, rgba8, strip) =
        if decoded.width <= MAX_ICON_SIDE && decoded.height <= MAX_ICON_SIDE {
            (decoded.width, decoded.height, decoded.rgba8, false)
        } else if decoded.width <= MAX_ICON_SIDE
            && decoded.height > decoded.width
            && decoded.height.is_multiple_of(decoded.width.max(1))
        {
            // A vertical animation strip (compass, clock): the flat inventory icon is the
            // strip's first recorded frame, never a guessed crop.
            let frame_bytes = decoded.width as usize * decoded.width as usize * 4;
            let frame = decoded.rgba8[..frame_bytes].to_vec().into_boxed_slice();
            (decoded.width, decoded.width, frame, true)
        } else {
            return None;
        };
    let sprite = IconSprite {
        width: u16::try_from(width).ok()?,
        height: u16::try_from(height).ok()?,
        rgba8: Arc::from(rgba8),
    };
    Some((sprite, strip))
}

/// Decodes the texture used by a compiled sprite source.
fn sprite_source(
    root: &Path,
    source: &assets::EntityAssetSource,
) -> Result<crate::image::DecodedTexture, AssetError> {
    let path = root.join(source.path.as_ref());
    let bytes = crate::entity::read_bounded_source(root, &path)?;
    if bytes.len() != source.source_bytes as usize
        || <[u8; 32]>::from(Sha256::digest(&bytes)) != source.source_sha256
    {
        return Err(AssetError::InvalidCompiledAssets {
            detail: "icon source changed after entity compilation".into(),
        });
    }
    crate::image::decode_texture_bytes(&path, &source.path, &bytes)
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_sprite_rereads_reject_changed_source_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("sprite.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([1; 4]))
            .save(&path)
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let source = assets::EntityAssetSource {
            path: "sprite.png".into(),
            source_bytes: bytes.len() as u32,
            source_sha256: Sha256::digest(bytes).into(),
        };
        image::RgbaImage::from_pixel(2, 1, image::Rgba([2; 4]))
            .save(&path)
            .unwrap();
        assert!(sprite_source(root.path(), &source).is_err());
    }
}
