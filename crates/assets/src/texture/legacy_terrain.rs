use super::{MAX_TILE_SIZE, TextureArray, TextureMip};
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

/// Rebuilds each terrain layer from its original bytes, leaving shared carried textures unchanged.
pub fn rebuild_legacy_terrain_mips(texture: &TextureArray) -> Result<TextureArray, AssetError> {
    let Some(base) = texture.mips.first() else {
        return Err(invalid("legacy terrain texture has no base level"));
    };
    if texture.layers == 0 || !base.size.is_power_of_two() || base.size > MAX_TILE_SIZE {
        return Err(invalid("legacy terrain array dimensions are unsupported"));
    }
    let layer_bytes = (base.size * base.size * 4) as usize;
    if layer_bytes.checked_mul(texture.layers as usize) != Some(base.rgba8.len()) {
        return Err(invalid(
            "legacy terrain array base has an invalid byte length",
        ));
    }
    let mut levels: Vec<Vec<u8>> = (0..=base.size.trailing_zeros())
        .map(|level| {
            Vec::with_capacity(((base.size >> level).pow(2) * 4) as usize * texture.layers as usize)
        })
        .collect();
    for layer in base.rgba8.chunks_exact(layer_bytes) {
        for (bytes, mip) in levels
            .iter_mut()
            .zip(build_legacy_terrain_mip_chain(layer, base.size)?)
        {
            bytes.extend_from_slice(&mip.rgba8);
        }
    }
    Ok(TextureArray {
        layers: texture.layers,
        mips: levels
            .into_iter()
            .enumerate()
            .map(|(level, bytes)| TextureMip {
                size: base.size >> level,
                rgba8: bytes.into_boxed_slice(),
            })
            .collect(),
    })
}

/// Reports malformed atlas inputs at the asset boundary.
fn invalid(detail: &str) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
