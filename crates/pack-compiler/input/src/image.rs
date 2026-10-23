use std::{
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use ::image::{ImageFormat, ImageReader, Limits};

use assets::{AssetError, TILE_SIZE, TextureMip, downsample_linear_premultiplied};

const MAX_TEXTURE_BYTES: usize = 1024 * 1024;
const MAX_DECODE_ALLOC: u64 = 256 * 1024;
const MAX_TEXTURE_DIMENSION: u32 = 4_096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedTexture {
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

/// Retains the first square frame of a valid horizontal or vertical strip.
/// Unsupported dimensions remain unchanged for the caller's validation.
pub fn retain_first_strip_frame(image: &mut DecodedTexture) {
    let side = image.width.min(image.height);
    let long_side = image.width.max(image.height);
    if side < TILE_SIZE
        || !side.is_power_of_two()
        || !long_side.is_multiple_of(side)
        || long_side / side > assets::texture::flipbook::MAX_FLIPBOOK_FRAMES as u32
        || image.width == image.height
    {
        return;
    }
    image.rgba8 = image
        .rgba8
        .chunks_exact(image.width as usize * 4)
        .take(side as usize)
        .flat_map(|row| row[..side as usize * 4].iter().copied())
        .collect();
    image.width = side;
    image.height = side;
}

pub fn diagnostic_pixels() -> Box<[u8]> {
    let mut pixels = Vec::with_capacity((TILE_SIZE * TILE_SIZE * 4) as usize);
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let color = if (x + y) % 2 == 0 {
                [255, 0, 255, 255]
            } else {
                [0, 0, 0, 255]
            };
            pixels.extend_from_slice(&color);
        }
    }
    pixels.into_boxed_slice()
}

pub fn decode_static_texture(path: &Path, key: &str) -> Result<Box<[u8]>, AssetError> {
    let decoded = decode_texture(path, key)?;
    if (decoded.width, decoded.height) != (TILE_SIZE, TILE_SIZE) {
        return Err(AssetError::WrongTextureDimensions {
            key: key.into(),
            path: path.to_path_buf(),
            width: decoded.width,
            height: decoded.height,
        });
    }
    Ok(decoded.rgba8)
}

pub fn decode_texture(path: &Path, key: &str) -> Result<DecodedTexture, AssetError> {
    decode_texture_file(path, key, false)
}

/// Decodes a declared animation strip with a dimension-derived allocation budget, retaining
/// its first square frame before returning it to a static model consumer.
pub fn decode_first_texture_frame(path: &Path, key: &str) -> Result<DecodedTexture, AssetError> {
    let mut decoded = decode_texture_file(path, key, true)?;
    retain_first_strip_frame(&mut decoded);
    Ok(decoded)
}

/// Reads a bounded texture source with the allocation policy of its consumer.
fn decode_texture_file(
    path: &Path,
    key: &str,
    first_frame: bool,
) -> Result<DecodedTexture, AssetError> {
    static_texture_format(path, key)?;
    let file = File::open(path).map_err(|source| AssetError::TextureIo {
        key: key.into(),
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.take((MAX_TEXTURE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetError::TextureIo {
            key: key.into(),
            path: path.to_path_buf(),
            source,
        })?;
    decode_texture_bytes_with_frame(path, key, &bytes, first_frame)
}

/// Decodes an already verified texture source within the image decoder's bounds.
pub fn decode_texture_bytes(
    path: &Path,
    key: &str,
    bytes: &[u8],
) -> Result<DecodedTexture, AssetError> {
    decode_texture_bytes_with_frame(path, key, bytes, false)
}

/// Validates dimensions before allowing a strip's decoder and output allocation.
fn decode_texture_bytes_with_frame(
    path: &Path,
    key: &str,
    bytes: &[u8],
    first_frame: bool,
) -> Result<DecodedTexture, AssetError> {
    let format = static_texture_format(path, key)?;
    if bytes.len() > MAX_TEXTURE_BYTES {
        return Err(AssetError::TextureTooLarge {
            key: key.into(),
            path: path.to_path_buf(),
            size: bytes.len(),
            max: MAX_TEXTURE_BYTES,
        });
    }

    let dimensions = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|source| AssetError::TextureDecode {
            key: key.into(),
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || dimensions.0 > MAX_TEXTURE_DIMENSION
        || dimensions.1 > MAX_TEXTURE_DIMENSION
    {
        return Err(AssetError::AnimationTextureDimensions {
            source_path: key.into(),
            width: dimensions.0,
            height: dimensions.1,
            detail: format!("dimensions must be within 1..={MAX_TEXTURE_DIMENSION}").into(),
        });
    }

    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_DIMENSION);
    limits.max_image_height = Some(MAX_TEXTURE_DIMENSION);
    limits.max_alloc = Some(if first_frame {
        // Reserve decoder working storage as well as the expanded RGBA output.
        MAX_DECODE_ALLOC + u64::from(dimensions.0) * u64::from(dimensions.1) * 8
    } else {
        MAX_DECODE_ALLOC
    });
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|source| AssetError::TextureDecode {
            key: key.into(),
            path: path.to_path_buf(),
            source: Box::new(source),
        })?;
    Ok(DecodedTexture {
        width: dimensions.0,
        height: dimensions.1,
        rgba8: decoded.into_rgba8().into_raw().into_boxed_slice(),
    })
}

pub fn normalize_texture_tile(
    mut rgba8: Box<[u8]>,
    mut size: u32,
    source_path: &str,
) -> Result<Box<[u8]>, AssetError> {
    let expected = size
        .checked_mul(size)
        .and_then(|pixels| pixels.checked_mul(4))
        .and_then(|bytes| usize::try_from(bytes).ok())
        .ok_or(AssetError::BlobSizeOverflow {
            section: "animation frame",
        })?;
    if rgba8.len() != expected {
        return Err(AssetError::AnimationTextureByteLength {
            source_path: source_path.into(),
            actual: rgba8.len(),
            expected,
        });
    }
    if size < TILE_SIZE || !size.is_power_of_two() {
        return Err(AssetError::AnimationTextureDimensions {
            source_path: source_path.into(),
            width: size,
            height: size,
            detail: format!("frame size must be a power of two at least {TILE_SIZE}").into(),
        });
    }
    while size > TILE_SIZE {
        rgba8 = downsample_linear_premultiplied(&rgba8, size);
        size /= 2;
    }
    Ok(rgba8)
}

pub fn build_texture_mip_chain(base: Box<[u8]>) -> Result<Box<[TextureMip]>, AssetError> {
    assets::build_texture_mip_chain(base, TILE_SIZE)
}

fn static_texture_format(path: &Path, key: &str) -> Result<ImageFormat, AssetError> {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("png") => Ok(ImageFormat::Png),
        Some(extension) if extension.eq_ignore_ascii_case("tga") => Ok(ImageFormat::Tga),
        _ => Err(AssetError::UnsupportedTextureFormat {
            key: key.into(),
            path: path.to_path_buf(),
        }),
    }
}

/// The upper-left `size / replicate` square of a `size`-pixel frame, enlarged by whole pixels to
/// at least one tile when that square is smaller. Unreplicated frames and frames outside the tile
/// size contract pass through unchanged for the caller to validate.
pub fn replicated_tile(frame: Vec<u8>, size: u32, replicate: u32) -> (Box<[u8]>, u32) {
    if replicate <= 1 || size < assets::TILE_SIZE || !size.is_power_of_two() {
        return (frame.into_boxed_slice(), size);
    }
    let crop = (size / replicate).max(1);
    let tile_size = if crop.is_power_of_two() && crop >= assets::TILE_SIZE {
        crop
    } else {
        assets::TILE_SIZE
    };
    let mut tile = Vec::with_capacity((tile_size * tile_size * 4) as usize);
    for y in 0..tile_size {
        let source_y = y * crop / tile_size;
        for x in 0..tile_size {
            let source_x = x * crop / tile_size;
            let start = ((source_y * size + source_x) * 4) as usize;
            tile.extend_from_slice(&frame[start..start + 4]);
        }
    }
    (tile.into_boxed_slice(), tile_size)
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::{build_texture_mip_chain, normalize_texture_tile};

    fn pattern(size: u32, seed: u32) -> Box<[u8]> {
        (0..size * size)
            .flat_map(|index| {
                let value = index.wrapping_mul(2_654_435_761).wrapping_add(seed);
                let alpha = if (index / 3 + seed).is_multiple_of(4) {
                    0
                } else {
                    255 - (value >> 29) as u8
                };
                [
                    (value >> 8) as u8,
                    (value >> 16) as u8,
                    (value >> 24) as u8,
                    alpha,
                ]
            })
            .collect()
    }

    // Pins the tile resampling and mip stages byte for byte across refactors.
    #[test]
    fn texture_tile_and_mip_stages_match_pinned_digest() {
        let mut hasher = Sha256::new();
        for base in [
            pattern(16, 1),
            normalize_texture_tile(pattern(64, 2), 64, "fixture").unwrap(),
            vec![200; 16 * 16 * 4].into_boxed_slice(),
        ] {
            for mip in build_texture_mip_chain(base).unwrap() {
                hasher.update(mip.size.to_le_bytes());
                hasher.update(&mip.rgba8);
            }
        }
        let digest = hasher.finalize();
        let hex = digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            hex,
            "8207a3e679420c9e8648f0efb9b605278c2c06b2552b3c2271be1155e58a233c"
        );
    }
}
