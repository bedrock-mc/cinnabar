use super::{MAX_TILE_SIZE, TextureMip};
use crate::AssetError;

/// Builds the native legacy terrain atlas's byte-space, unassociated RGBA mips.
/// Each level averages the original pixels independently, not the previous mip.
///
/// Vanilla's atlas mip update samples a `2^level` square, normalizes bytes, averages all four channels equally and
/// truncates the final byte. It neither linearizes RGB nor rescales cutout alpha.
/// The compiler uses separate world-leaf layers so carried/shared art retains
/// its existing mip contract.
pub fn build_legacy_terrain_mip_chain(
    base: &[u8],
    tile_size: u32,
) -> Result<Box<[TextureMip]>, AssetError> {
    if !tile_size.is_power_of_two() || tile_size > MAX_TILE_SIZE {
        return Err(invalid("legacy terrain tile size is unsupported"));
    }
    if base.len() != (tile_size * tile_size * 4) as usize {
        return Err(invalid("legacy terrain base has an invalid byte length"));
    }
    let mut mips = Vec::with_capacity(tile_size.trailing_zeros() as usize + 1);
    let mut size = tile_size;
    while size != 0 {
        let span = tile_size / size;
        let weight = 1.0 / (span * span) as f32;
        let mut rgba8 = Vec::with_capacity((size * size * 4) as usize);
        for y in 0..size {
            for x in 0..size {
                let mut sums = [0.0_f32; 4];
                for dy in 0..span {
                    for dx in 0..span {
                        let offset = (((y * span + dy) * tile_size + x * span + dx) * 4) as usize;
                        for channel in 0..sums.len() {
                            sums[channel] += f32::from(base[offset + channel]) * (1.0 / 255.0);
                        }
                    }
                }
                rgba8.extend(sums.map(|sum| (sum * weight * 255.0) as u8));
            }
        }
        mips.push(TextureMip {
            size,
            rgba8: rgba8.into_boxed_slice(),
        });
        size /= 2;
    }
    Ok(mips.into_boxed_slice())
}

fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
