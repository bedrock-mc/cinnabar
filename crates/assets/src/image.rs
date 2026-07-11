use std::{
    collections::BTreeSet,
    fs::File,
    io::{Cursor, Read},
    path::Path,
};

use ::image::{ImageFormat, ImageReader, Limits};

use crate::AssetError;

pub const TILE_SIZE: u32 = 16;
pub const MIP_COUNT: u32 = 5;

const MAX_TEXTURE_BYTES: usize = 1024 * 1024;
const MAX_DECODE_ALLOC: u64 = 256 * 1024;
const MAX_TEXTURE_DIMENSION: u32 = 4_096;
const ALPHA_TEST_THRESHOLD: u8 = 128;
const ALPHA_SCALE_FRACTION_BITS: u32 = 16;
const ALPHA_SCALE_MAX: u32 = 16 << ALPHA_SCALE_FRACTION_BITS;
const ALPHA_SCALE_SEARCH_STEPS: usize = 21;

/// One mip level containing every array layer in layer-major RGBA8 order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureMip {
    pub size: u32,
    pub rgba8: Box<[u8]>,
}

/// Equal-sized 16x16 texture-array layers with independent mip chains.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureArray {
    pub layers: u32,
    pub mips: Box<[TextureMip]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedTexture {
    pub width: u32,
    pub height: u32,
    pub rgba8: Box<[u8]>,
}

pub(crate) fn diagnostic_pixels() -> Box<[u8]> {
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

pub(crate) fn decode_static_texture(path: &Path, key: &str) -> Result<Box<[u8]>, AssetError> {
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

pub(crate) fn decode_texture(path: &Path, key: &str) -> Result<DecodedTexture, AssetError> {
    let format = static_texture_format(path, key)?;
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
    if bytes.len() > MAX_TEXTURE_BYTES {
        return Err(AssetError::TextureTooLarge {
            key: key.into(),
            path: path.to_path_buf(),
            size: bytes.len(),
            max: MAX_TEXTURE_BYTES,
        });
    }

    let dimensions = ImageReader::with_format(Cursor::new(&bytes), format)
        .into_dimensions()
        .map_err(|source| AssetError::TextureDecode {
            key: key.into(),
            path: path.to_path_buf(),
            source,
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

    let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_DIMENSION);
    limits.max_image_height = Some(MAX_TEXTURE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|source| AssetError::TextureDecode {
            key: key.into(),
            path: path.to_path_buf(),
            source,
        })?;
    Ok(DecodedTexture {
        width: dimensions.0,
        height: dimensions.1,
        rgba8: decoded.into_rgba8().into_raw().into_boxed_slice(),
    })
}

pub(crate) fn normalize_texture_tile(
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

pub(crate) fn build_texture_mip_chain(base: Box<[u8]>) -> Result<Box<[TextureMip]>, AssetError> {
    let has_covered = base
        .chunks_exact(4)
        .any(|pixel| pixel[3] >= ALPHA_TEST_THRESHOLD);
    let has_uncovered = base
        .chunks_exact(4)
        .any(|pixel| pixel[3] < ALPHA_TEST_THRESHOLD);
    let cutout_layers = if has_covered && has_uncovered {
        BTreeSet::from([0])
    } else {
        BTreeSet::new()
    };
    let texture = build_texture_array(&[base], &cutout_layers, &BTreeSet::new())?;
    Ok(texture.mips)
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

pub(crate) fn build_texture_array(
    base_layers: &[Box<[u8]>],
    cutout_layers: &BTreeSet<u32>,
    overlay_mask_layers: &BTreeSet<u32>,
) -> Result<TextureArray, AssetError> {
    let expected_base = (TILE_SIZE * TILE_SIZE * 4) as usize;
    if base_layers.is_empty() {
        return Err(invalid("texture array has no diagnostic layer"));
    }
    for (layer, pixels) in base_layers.iter().enumerate() {
        if pixels.len() != expected_base {
            return Err(invalid(format!(
                "base layer {layer} has {} bytes, expected {expected_base}",
                pixels.len()
            )));
        }
    }
    if let Some(&layer) = cutout_layers
        .iter()
        .find(|&&layer| layer as usize >= base_layers.len())
    {
        return Err(invalid(format!(
            "cutout layer {layer} is outside {} base layers",
            base_layers.len()
        )));
    }
    if let Some(&layer) = overlay_mask_layers
        .iter()
        .find(|&&layer| layer as usize >= base_layers.len())
    {
        return Err(invalid(format!(
            "overlay-mask layer {layer} is outside {} base layers",
            base_layers.len()
        )));
    }

    let layers = u32::try_from(base_layers.len()).map_err(|_| AssetError::BlobSizeOverflow {
        section: "texture layer count",
    })?;
    let base_survivors = base_layers
        .iter()
        .enumerate()
        .map(|(layer, pixels)| {
            cutout_layers
                .contains(&u32::try_from(layer).expect("layer count converted above"))
                .then(|| alpha_survivors(pixels))
        })
        .collect::<Vec<_>>();
    let mut per_layer = base_layers.to_vec();
    let mut mips = Vec::with_capacity(MIP_COUNT as usize);
    let mut size = TILE_SIZE;
    loop {
        let bytes_per_layer = (size * size * 4) as usize;
        let total =
            bytes_per_layer
                .checked_mul(per_layer.len())
                .ok_or(AssetError::BlobSizeOverflow {
                    section: "texture mip",
                })?;
        let mut rgba8 = Vec::with_capacity(total);
        for (layer, pixels) in per_layer.iter().enumerate() {
            if size < TILE_SIZE
                && let Some(base_survivors) = base_survivors[layer]
            {
                let mip_pixels =
                    usize::try_from(size * size).expect("bounded texture mip fits usize");
                let target = (base_survivors * mip_pixels + 128) / 256;
                let mut corrected = pixels.to_vec();
                preserve_alpha_coverage(&mut corrected, target);
                rgba8.extend_from_slice(&corrected);
                continue;
            }
            rgba8.extend_from_slice(pixels);
        }
        mips.push(TextureMip {
            size,
            rgba8: rgba8.into_boxed_slice(),
        });
        if size == 1 {
            break;
        }
        let target_size = size / 2;
        per_layer = per_layer
            .iter()
            .enumerate()
            .map(|(layer, pixels)| {
                let layer = u32::try_from(layer).expect("texture layer count is bounded");
                if overlay_mask_layers.contains(&layer) {
                    downsample_linear_unassociated(pixels, size)
                } else {
                    downsample_linear_premultiplied(pixels, size)
                }
            })
            .collect();
        size = target_size;
    }

    Ok(TextureArray {
        layers,
        mips: mips.into_boxed_slice(),
    })
}

fn alpha_survivors(rgba: &[u8]) -> usize {
    rgba.chunks_exact(4)
        .filter(|pixel| pixel[3] >= ALPHA_TEST_THRESHOLD)
        .count()
}

fn scaled_alpha(alpha: u8, scale: u32) -> u8 {
    let rounding = 1 << (ALPHA_SCALE_FRACTION_BITS - 1);
    ((u32::from(alpha) * scale + rounding) >> ALPHA_SCALE_FRACTION_BITS).min(255) as u8
}

fn scaled_survivors(rgba: &[u8], scale: u32) -> usize {
    rgba.chunks_exact(4)
        .filter(|pixel| scaled_alpha(pixel[3], scale) >= ALPHA_TEST_THRESHOLD)
        .count()
}

fn preserve_alpha_coverage(rgba: &mut [u8], target: usize) {
    let mut lower = 0_u32;
    let mut upper = ALPHA_SCALE_MAX + 1;
    for _ in 0..ALPHA_SCALE_SEARCH_STEPS {
        let middle = lower + (upper - lower) / 2;
        if scaled_survivors(rgba, middle) >= target {
            upper = middle;
        } else {
            lower = middle + 1;
        }
    }
    debug_assert_eq!(lower, upper);
    let upper_scale = lower.min(ALPHA_SCALE_MAX);
    let lower_scale = upper_scale.saturating_sub(1);
    let upper_error = scaled_survivors(rgba, upper_scale).abs_diff(target);
    let lower_error = scaled_survivors(rgba, lower_scale).abs_diff(target);
    let candidate = if lower_error <= upper_error {
        lower_scale
    } else {
        upper_scale
    };
    let survivor_count = scaled_survivors(rgba, candidate);
    let scale = smallest_scale_for_survivors(rgba, survivor_count, candidate);
    for pixel in rgba.chunks_exact_mut(4) {
        pixel[3] = scaled_alpha(pixel[3], scale);
    }
}

fn smallest_scale_for_survivors(rgba: &[u8], survivors: usize, upper_bound: u32) -> u32 {
    const SURVIVOR_NUMERATOR: u32 = ((ALPHA_TEST_THRESHOLD as u32) << ALPHA_SCALE_FRACTION_BITS)
        - (1 << (ALPHA_SCALE_FRACTION_BITS - 1));
    let mut smallest = if survivors == 0 { 0 } else { upper_bound };
    for alpha in rgba.chunks_exact(4).map(|pixel| pixel[3]) {
        if alpha == 0 {
            continue;
        }
        let threshold = SURVIVOR_NUMERATOR.div_ceil(u32::from(alpha));
        if threshold <= upper_bound && scaled_survivors(rgba, threshold) == survivors {
            smallest = smallest.min(threshold);
        }
    }
    smallest
}

fn downsample_linear_premultiplied(source: &[u8], source_size: u32) -> Box<[u8]> {
    let target_size = source_size / 2;
    let mut target = Vec::with_capacity((target_size * target_size * 4) as usize);
    for y in 0..target_size {
        for x in 0..target_size {
            let mut premultiplied = [0.0_f32; 3];
            let mut alpha_sum = 0.0_f32;
            for offset_y in 0..2 {
                for offset_x in 0..2 {
                    let source_x = x * 2 + offset_x;
                    let source_y = y * 2 + offset_y;
                    let offset = ((source_y * source_size + source_x) * 4) as usize;
                    let alpha = f32::from(source[offset + 3]) / 255.0;
                    alpha_sum += alpha;
                    for channel in 0..3 {
                        premultiplied[channel] += srgb_to_linear(source[offset + channel]) * alpha;
                    }
                }
            }
            let alpha = alpha_sum / 4.0;
            for value in premultiplied {
                let linear = if alpha_sum > 0.0 {
                    value / alpha_sum
                } else {
                    0.0
                };
                target.push(linear_to_srgb(linear));
            }
            target.push(float_to_byte(alpha));
        }
    }
    target.into_boxed_slice()
}

fn downsample_linear_unassociated(source: &[u8], source_size: u32) -> Box<[u8]> {
    let target_size = source_size / 2;
    let mut target = Vec::with_capacity((target_size * target_size * 4) as usize);
    for y in 0..target_size {
        for x in 0..target_size {
            let mut linear_sum = [0.0_f32; 3];
            let mut alpha_sum = 0.0_f32;
            for offset_y in 0..2 {
                for offset_x in 0..2 {
                    let source_x = x * 2 + offset_x;
                    let source_y = y * 2 + offset_y;
                    let offset = ((source_y * source_size + source_x) * 4) as usize;
                    alpha_sum += f32::from(source[offset + 3]) / 255.0;
                    for channel in 0..3 {
                        linear_sum[channel] += srgb_to_linear(source[offset + channel]);
                    }
                }
            }
            for value in linear_sum {
                target.push(linear_to_srgb(value / 4.0));
            }
            target.push(float_to_byte(alpha_sum / 4.0));
        }
    }
    target.into_boxed_slice()
}

fn srgb_to_linear(value: u8) -> f32 {
    let value = f32::from(value) / 255.0;
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let srgb = if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    float_to_byte(srgb)
}

fn float_to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{downsample_linear_premultiplied, downsample_linear_unassociated};

    #[test]
    fn transparent_colour_does_not_bleed_into_linear_mips() {
        let source = [255, 0, 0, 255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0];

        assert_eq!(
            downsample_linear_premultiplied(&source, 2).as_ref(),
            [255, 0, 0, 64]
        );
    }

    #[test]
    fn tint_mask_mips_preserve_rgb_where_alpha_is_zero() {
        let source = [
            100, 50, 25, 0, 100, 50, 25, 0, 200, 200, 200, 255, 200, 200, 200, 255,
        ];

        let mip = downsample_linear_unassociated(&source, 2);
        assert!(
            mip[0] > 100,
            "both base and overlay RGB contribute to the mip"
        );
        assert_eq!(mip[3], 128, "alpha remains only the overlay weight");
    }
}
