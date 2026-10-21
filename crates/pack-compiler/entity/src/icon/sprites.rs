//! Source-deduplicated flat icon baking and bounded texture decoding.

use std::{collections::BTreeMap, path::Path, sync::Arc};

use assets::{AssetError, CompiledEntityAssets, IconSprite, MAX_ICON_SIDE};
use sha2::{Digest, Sha256};

#[derive(Default)]
pub(super) struct SpriteBaker {
    by_source: BTreeMap<u32, Option<u32>>,
    pub(super) animation_strips: usize,
    pub(super) skipped_oversized: usize,
    pub(super) sources: BTreeMap<Box<str>, u32>,
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
        let raw = assets::facts::pack::leather_sprite(&source.path)
            .then(|| bounded_sprite(decoded.clone()).map(|(sprite, _)| sprite))
            .flatten();
        if assets::facts::pack::leather_sprite(&source.path) {
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
        let raw_index = match raw {
            Some(raw) => {
                let raw_index = sprites.len() as u32;
                sprites.push(raw);
                raw_index
            }
            None => index,
        };
        self.sources
            .insert(source.path.to_ascii_lowercase().into(), raw_index);
        self.by_source.insert(source_index, Some(index));
        Ok(Some(index))
    }
}

/// Bounds a decoded texture to a flat icon: as-is within `MAX_ICON_SIDE`, else a vertical
/// animation strip's first frame (`true`); anything else is refused.
pub(super) fn bounded_sprite(
    decoded: pack_compile_input::image::DecodedTexture,
) -> Option<(IconSprite, bool)> {
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

/// Most frames baked from one dial strip.
const MAX_DIAL_FRAMES: u32 = 256;

/// Every frame of the dial strip at `path`, top to bottom. Empty when the pack lacks the strip
/// or it is not a vertical run of square frames within `MAX_ICON_SIDE`, so the item keeps its
/// static icon.
pub(super) fn strip_frames(
    root: &Path,
    compiled: &CompiledEntityAssets,
    path: &str,
) -> Result<Vec<IconSprite>, AssetError> {
    let Some(source) = compiled
        .sources
        .iter()
        .find(|source| source.path.as_ref() == path)
    else {
        return Ok(Vec::new());
    };
    Ok(split_strip(sprite_source(root, source)?))
}

/// Splits a vertical strip of square frames into one sprite per frame.
fn split_strip(decoded: pack_compile_input::image::DecodedTexture) -> Vec<IconSprite> {
    let side = decoded.width;
    if side == 0
        || side > MAX_ICON_SIDE
        || decoded.height <= side
        || !decoded.height.is_multiple_of(side)
        || decoded.height / side > MAX_DIAL_FRAMES
    {
        return Vec::new();
    }
    let Ok(side16) = u16::try_from(side) else {
        return Vec::new();
    };
    decoded
        .rgba8
        .chunks_exact(side as usize * side as usize * 4)
        .map(|frame| IconSprite {
            width: side16,
            height: side16,
            rgba8: Arc::from(frame),
        })
        .collect()
}

/// Decodes the texture used by a compiled sprite source.
fn sprite_source(
    root: &Path,
    source: &assets::EntityAssetSource,
) -> Result<pack_compile_input::image::DecodedTexture, AssetError> {
    let path = root.join(source.path.as_ref());
    let bytes = pack_compile_input::source::read_bounded_source(root, &path)?;
    if bytes.len() != source.source_bytes as usize
        || <[u8; 32]>::from(Sha256::digest(&bytes)) != source.source_sha256
    {
        return Err(AssetError::InvalidCompiledAssets {
            detail: "icon source changed after entity compilation".into(),
        });
    }
    pack_compile_input::image::decode_texture_bytes(&path, &source.path, &bytes)
}

#[cfg(test)]
mod review_tests {
    use super::*;

    #[test]
    fn dial_strips_split_into_square_frames_in_order() {
        let side = 2u32;
        let rgba8: Vec<u8> = (0..3u8)
            .flat_map(|frame| std::iter::repeat_n(frame, (side * side * 4) as usize))
            .collect();
        let frames = split_strip(pack_compile_input::image::DecodedTexture {
            width: side,
            height: side * 3,
            rgba8: rgba8.into(),
        });
        assert_eq!(frames.len(), 3);
        for (index, frame) in frames.iter().enumerate() {
            assert_eq!((frame.width, frame.height), (2, 2));
            assert!(frame.rgba8.iter().all(|&byte| byte == index as u8));
        }
        let square = split_strip(pack_compile_input::image::DecodedTexture {
            width: side,
            height: side,
            rgba8: vec![0; 16].into(),
        });
        assert!(square.is_empty(), "a single frame is not a strip");
    }

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
