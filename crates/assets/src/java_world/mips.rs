//! Linear-light mip averaging with the pinned renderer's channel quantization.

use std::sync::OnceLock;

/// Halves a bounded square image, averaging RGB through a ten-bit linear channel table.
pub fn mean_mip(source: &[u8], side: u32) -> Option<Box<[u8]>> {
    if side < 2 || !side.is_power_of_two() || source.len() != side as usize * side as usize * 4 {
        return None;
    }
    let (decode, encode) = tables();
    let mut target = Vec::with_capacity(source.len() / 4);
    for y in 0..side / 2 {
        for x in 0..side / 2 {
            let offsets =
                [0, 1, side, side + 1].map(|offset| ((y * 2 * side + x * 2 + offset) * 4) as usize);
            for channel in 0..3 {
                let sum: usize = offsets
                    .iter()
                    .map(|offset| usize::from(decode[source[offset + channel] as usize]))
                    .sum();
                target.push(encode[sum / 4]);
            }
            target.push(
                (offsets
                    .iter()
                    .map(|offset| u16::from(source[offset + 3]))
                    .sum::<u16>()
                    / 4) as u8,
            );
        }
    }
    Some(target.into_boxed_slice())
}

/// Derives both transfer tables once from their channel depths.
fn tables() -> &'static ([u16; 256], [u8; 1024]) {
    static TABLES: OnceLock<([u16; 256], [u8; 1024])> = OnceLock::new();
    TABLES.get_or_init(|| {
        (
            std::array::from_fn(|i| {
                let x = i as f32 / 255.0;
                let linear = if x >= 0.04045 {
                    ((f64::from(x) + 0.055) / 1.055).powf(2.4) as f32
                } else {
                    x / 12.92
                };
                (linear * 1023.0).round() as u16
            }),
            std::array::from_fn(|i| {
                let x = i as f32 / 1023.0;
                let encoded = if x >= 0.0031308 {
                    (1.055 * f64::from(x).powf(1.0 / 2.4) - 0.055) as f32
                } else {
                    12.92 * x
                };
                (encoded * 255.0).round() as u8
            }),
        )
    })
}

/// Generates one terrain layer with either mean or coverage-preserving cutout levels.
pub fn terrain_layer(
    base: &[u8],
    side: u32,
    levels: usize,
    cutout: bool,
    dark: bool,
) -> Option<Box<[crate::TextureMip]>> {
    if side == 0
        || !side.is_power_of_two()
        || base.len() != side as usize * side as usize * 4
        || levels == 0
        || levels > side.ilog2() as usize + 1
    {
        return None;
    }
    let mut pixels = base.to_vec().into_boxed_slice();
    if cutout {
        fill_transparent(&mut pixels, side, dark);
    }
    let desired = coverage(&pixels, side, 1.0);
    let mut mips = vec![crate::TextureMip {
        size: side,
        rgba8: pixels,
    }];
    for level in 1..levels {
        let previous = &mips[level - 1];
        let mut pixels = if dark {
            dark_mean_mip(&previous.rgba8, previous.size)
        } else {
            mean_mip(&previous.rgba8, previous.size)?
        };
        let size = previous.size / 2;
        if cutout {
            scale_coverage(&mut pixels, size, desired);
        }
        mips.push(crate::TextureMip {
            size,
            rgba8: pixels,
        });
    }
    Some(mips.into_boxed_slice())
}

/// Fills transparent RGB from a nearby covered pixel or the layer's darkest covered colour.
fn fill_transparent(pixels: &mut [u8], side: u32, dark: bool) {
    let count = side as usize * side as usize;
    if dark {
        let mut darkest = [255_u8; 3];
        let mut brightness = u16::MAX;
        for x in 0..side as usize {
            for y in 0..side as usize {
                let pixel = &pixels[(y * side as usize + x) * 4..][..4];
                let sum = pixel[..3].iter().map(|&v| u16::from(v)).sum();
                if pixel[3] != 0 && sum < brightness {
                    brightness = sum;
                    darkest.copy_from_slice(&pixel[..3]);
                }
            }
        }
        for pixel in pixels.as_chunks_mut::<4>().0 {
            if pixel[3] == 0 {
                for i in 0..3 {
                    pixel[i] = (u16::from(darkest[i]) * 3 / 4) as u8;
                }
            }
        }
        return;
    }
    let mut nearest = vec![None; count];
    let mut queue = std::collections::VecDeque::new();
    for x in 0..side as usize {
        for y in 0..side as usize {
            let index = y * side as usize + x;
            if pixels[index * 4 + 3] != 0 {
                nearest[index] = Some(index);
                queue.push_back(index);
            }
        }
    }
    while let Some(index) = queue.pop_front() {
        let x = (index % side as usize) as i32;
        let y = (index / side as usize) as i32;
        for [dx, dy] in [[1, 0], [-1, 0], [0, 1], [0, -1]] {
            let (x, y) = (x + dx, y + dy);
            if x < 0 || y < 0 || x >= side as i32 || y >= side as i32 {
                continue;
            }
            let neighbour = y as usize * side as usize + x as usize;
            if nearest[neighbour].is_none() {
                nearest[neighbour] = nearest[index];
                queue.push_back(neighbour);
            }
        }
    }
    for (index, source) in nearest.into_iter().enumerate() {
        if pixels[index * 4 + 3] == 0
            && let Some(source) = source
        {
            let rgb: [u8; 3] = pixels[source * 4..source * 4 + 3]
                .try_into()
                .expect("RGB channels");
            pixels[index * 4..index * 4 + 3].copy_from_slice(&rgb);
        }
    }
}

/// Measures bilinearly filtered alpha coverage with sixteen samples per adjacent texel square.
fn coverage(pixels: &[u8], side: u32, scale: f32) -> f32 {
    if side < 2 {
        return f32::NAN;
    }
    let mut count = 0_u32;
    for y in 0..side - 1 {
        for x in 0..side - 1 {
            let a = [0, 1, side, side + 1].map(|offset| {
                (f32::from(pixels[((y * side + x + offset) * 4 + 3) as usize]) / 255.0 * scale)
                    .clamp(0.0, 1.0)
            });
            for sy in 0..4 {
                for sx in 0..4 {
                    let (fx, fy) = ((sx as f32 + 0.5) / 4.0, (sy as f32 + 0.5) / 4.0);
                    let alpha = a[0] * (1.0 - fx) * (1.0 - fy)
                        + a[1] * fx * (1.0 - fy)
                        + a[2] * (1.0 - fx) * fy
                        + a[3] * fx * fy;
                    count += u32::from(alpha > 0.5);
                }
            }
        }
    }
    count as f32 / (16 * (side - 1).pow(2)) as f32
}

/// Chooses a bounded alpha scale, keeping the earliest equally good candidate.
fn scale_coverage(pixels: &mut [u8], side: u32, desired: f32) {
    let (mut low, mut high, mut scale, mut best, mut error) = (0.0, 4.0, 1.0, 1.0, f32::MAX);
    for _ in 0..5 {
        let actual = coverage(pixels, side, scale);
        let next_error = (actual - desired).abs();
        if next_error < error {
            best = scale;
            error = next_error;
        }
        if actual < desired {
            low = scale;
        } else if actual > desired {
            high = scale;
        } else {
            break;
        }
        scale = (low + high) * 0.5;
    }
    for pixel in pixels.as_chunks_mut::<4>().0 {
        pixel[3] = ((f32::from(pixel[3]) / 255.0 * best + 0.025).clamp(0.0, 1.0) * 255.0) as u8;
    }
}

/// Dark cutouts average only covered channels, including alpha, through the linear tables.
fn dark_mean_mip(source: &[u8], side: u32) -> Box<[u8]> {
    let (decode, encode) = tables();
    let mut result = Vec::with_capacity(source.len() / 4);
    for y in 0..side / 2 {
        for x in 0..side / 2 {
            let offsets =
                [0, 1, side, side + 1].map(|offset| ((y * 2 * side + x * 2 + offset) * 4) as usize);
            for channel in 0..4 {
                let total = offsets
                    .iter()
                    .filter(|&&offset| source[offset + 3] != 0)
                    .map(|offset| f32::from(decode[source[offset + channel] as usize]) / 1023.0)
                    .sum::<f32>()
                    / 4.0;
                result.push(encode[(total * 1023.0).floor() as usize]);
            }
        }
    }
    result.into_boxed_slice()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rgb_averages_in_linear_light_while_alpha_stays_linear_in_bytes() {
        let source = [
            0, 0, 0, 0, 255, 255, 255, 100, 0, 0, 0, 200, 255, 255, 255, 255,
        ];
        let mip = mean_mip(&source, 2).unwrap();
        // The ten-bit average rounds down before the final channel lookup.
        assert_eq!(&mip[..3], &[187; 3]);
        assert_eq!(mip[3], 138);
        assert!(mean_mip(&source[..12], 2).is_none());
    }

    #[test]
    fn cutout_mips_do_not_bleed_hidden_rgb_into_covered_edges() {
        let base = (0..16)
            .flat_map(|index| {
                if index < 8 {
                    [255, 0, 0, 255]
                } else {
                    [0, 0, 255, 0]
                }
            })
            .collect::<Vec<_>>();
        let chain = terrain_layer(&base, 4, 3, true, false).unwrap();
        for level in &chain {
            assert!(
                level
                    .rgba8
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| pixel[1] == 0 && pixel[2] == 0)
            );
        }
        assert_eq!(&base[8 * 4..][..4], &[0, 0, 255, 0]);
        let dark = dark_mean_mip(
            &[255, 0, 0, 255, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0],
            2,
        );
        assert_eq!(&dark[1..3], &[0, 0]);
        assert!(dark[0] > 0 && dark[0] < 255);
    }
}
