//! Admitted terrain rectangles retain their own pixel scale inside array layers.

use super::{DecodedTexture, resample_square};

/// Admits the first square of a vertical strip; horizontal rectangles retain their width.
pub(in super::super) fn admit_static_rectangle(texture: &mut DecodedTexture) {
    if texture.height > texture.width {
        texture.height = texture.width;
        let mut pixels = std::mem::take(&mut texture.rgba8).into_vec();
        pixels.truncate((texture.width * texture.height * 4) as usize);
        texture.rgba8 = pixels.into_boxed_slice();
    }
}

/// Builds each source mip before expanding it into the physical array mip.
pub(in super::super) fn source_mip_chain(
    texture: &DecodedTexture,
    tile: u32,
) -> Option<([u16; 2], Box<[assets::TextureMip]>)> {
    let dimensions = [texture.width, texture.height];
    if dimensions.iter().any(|size| !size.is_power_of_two()) {
        let base = resample_square(texture, tile);
        return Some((
            [tile as u16; 2],
            assets::build_legacy_terrain_mip_chain(&base, tile).ok()?,
        ));
    }
    let bias = dimensions
        .into_iter()
        .max()?
        .ilog2()
        .saturating_sub(tile.ilog2());
    let admitted = dimensions.map(|size| (size >> bias).max(1) as u16);
    let mips = (0..=tile.ilog2())
        .map(|level| {
            let (rgba8, [width, height]) =
                assets::legacy_terrain_mip(&texture.rgba8, dimensions, level + bias).ok()?;
            let size = tile >> level;
            Some(assets::TextureMip {
                size,
                rgba8: resample_square(
                    &DecodedTexture {
                        width,
                        height,
                        rgba8,
                    },
                    size,
                ),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some((admitted, mips.into_boxed_slice()))
}
