use std::collections::BTreeSet;

use crate::AssetError;

mod legacy_terrain;
pub use legacy_terrain::build_legacy_terrain_mip_chain;

pub const TILE_SIZE: u32 = 16;
pub const MIP_COUNT: u32 = 5;
/// Optional authored material metadata in otherwise unused packed texture-reference bits.
pub const PBR_REF_COLOR: u32 = 1 << 11;
pub const PBR_REF_NORMAL: u32 = 1 << 12;
/// Only spatially varying authored height enables displacement sampling.
pub const PBR_REF_HEIGHT: u32 = 1 << 13;
pub const PBR_REF_MATERIAL: u32 = 1 << 14;
pub const PBR_REF_LABPBR: u32 = 1 << 15;
pub const PBR_REF_OCCLUSION: u32 = 1 << 16;
pub const PBR_REF_SUBSURFACE: u32 = 1 << 17;
/// Authored height spans this many block units in both derived normals and parallax tracing.
pub const PBR_HEIGHT_SCALE: f32 = 0.035;
pub const PBR_TILE_SIZE: u32 = 512;
/// Largest square layer a runtime overlay page may use.
pub const MAX_TILE_SIZE: u32 = 256;
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

/// Equal-sized square texture-array layers with independent mip chains.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureArray {
    pub layers: u32,
    pub mips: Box<[TextureMip]>,
}

/// Builds one layer's mip chain down to 1x1 from a square power-of-two base.
/// Cutout coverage is preserved across mips when the base mixes covered and
/// uncovered texels.
pub fn build_texture_mip_chain(
    base: Box<[u8]>,
    tile_size: u32,
) -> Result<Box<[TextureMip]>, AssetError> {
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
    let texture = build_texture_array(&[base], tile_size, &cutout_layers)?;
    Ok(texture.mips)
}

fn build_texture_array(
    base_layers: &[Box<[u8]>],
    tile_size: u32,
    cutout_layers: &BTreeSet<u32>,
) -> Result<TextureArray, AssetError> {
    if !tile_size.is_power_of_two() || tile_size > MAX_TILE_SIZE {
        return Err(invalid(format!(
            "texture tile size {tile_size} is unsupported"
        )));
    }
    let expected_base = (tile_size * tile_size * 4) as usize;
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
    let mut mips = Vec::with_capacity(tile_size.trailing_zeros() as usize + 1);
    let mut size = tile_size;
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
            if size < tile_size
                && let Some(base_survivors) = base_survivors[layer]
            {
                let mip_pixels =
                    usize::try_from(size * size).expect("bounded texture mip fits usize");
                let base_pixels = (tile_size * tile_size) as usize;
                let target = (base_survivors * mip_pixels + base_pixels / 2) / base_pixels;
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
            .map(|pixels| downsample_linear_premultiplied(pixels, size))
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

#[cfg(test)]
thread_local! { static COVERAGE_PIXEL_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

fn scaled_survivors(rgba: &[u8], scale: u32) -> usize {
    rgba.chunks_exact(4)
        .filter(|pixel| {
            #[cfg(test)]
            COVERAGE_PIXEL_VISITS.with(|visits| visits.set(visits.get() + 1));
            scaled_alpha(pixel[3], scale) >= ALPHA_TEST_THRESHOLD
        })
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
    let mut histogram = [0usize; 256];
    for pixel in rgba.chunks_exact(4) {
        histogram[usize::from(pixel[3])] += 1;
    }
    let mut suffix = [0usize; 257];
    for alpha in (0..histogram.len()).rev() {
        suffix[alpha] = suffix[alpha + 1] + histogram[alpha];
    }
    for alpha in 1..=u8::MAX {
        if histogram[usize::from(alpha)] == 0 {
            continue;
        }
        let threshold = SURVIVOR_NUMERATOR.div_ceil(u32::from(alpha));
        if threshold > upper_bound {
            continue;
        }
        let cutoff = SURVIVOR_NUMERATOR.div_ceil(threshold) as usize;
        let count = suffix.get(cutoff).copied().unwrap_or(0);
        if count == survivors {
            smallest = smallest.min(threshold);
        }
    }
    smallest
}

/// Halves a square RGBA8 tile in linear light with alpha-weighted colour, so
/// transparent texels never bleed into the result.
#[must_use]
pub fn downsample_linear_premultiplied(source: &[u8], source_size: u32) -> Box<[u8]> {
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
    #[test]
    fn review_alpha_scale_selection_does_not_rescan_for_every_pixel() {
        let rgba = [0, 0, 0, 128].repeat(4096);
        super::COVERAGE_PIXEL_VISITS.with(|visits| visits.set(0));
        let scale =
            super::smallest_scale_for_survivors(&rgba, 4096, 1 << super::ALPHA_SCALE_FRACTION_BITS);
        assert_eq!(super::scaled_alpha(128, scale), 128);
        assert!(super::COVERAGE_PIXEL_VISITS.with(|visits| visits.get()) <= 4096 * 256);
    }

    use super::{ALPHA_TEST_THRESHOLD, build_texture_mip_chain, downsample_linear_premultiplied};

    // Coverage targets scale with the base size, so larger cutout tiles keep their share.
    #[test]
    fn cutout_coverage_is_preserved_for_larger_tiles() {
        let base = (0..32 * 32)
            .flat_map(|index| [90, 90, 90, if index % 3 == 0 { 255 } else { 0 }])
            .collect::<Box<[u8]>>();
        let mips = build_texture_mip_chain(base, 32).expect("mip chain");
        assert_eq!(mips.len(), 6);
        let covered = mips[2]
            .rgba8
            .chunks_exact(4)
            .filter(|pixel| pixel[3] >= ALPHA_TEST_THRESHOLD)
            .count();
        assert!(
            covered.abs_diff(21) <= 1,
            "a third of the 8x8 mip stays covered: {covered}"
        );
    }

    #[test]
    fn transparent_colour_does_not_bleed_into_linear_mips() {
        let source = [255, 0, 0, 255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0];

        assert_eq!(
            downsample_linear_premultiplied(&source, 2).as_ref(),
            [255, 0, 0, 64]
        );
    }
}
