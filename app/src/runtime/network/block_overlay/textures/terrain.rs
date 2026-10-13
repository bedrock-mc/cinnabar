//! Admitted terrain rectangles retain their own pixel scale inside array layers.

use super::{DecodedTexture, resample_square};

/// Returns reduced dimensions while retaining the minimum size of each terrain axis.
pub(in super::super) fn reduced_dimensions(dimensions: [u32; 2], side: u32) -> [u32; 2] {
    let longest = dimensions[0].max(dimensions[1]);
    if longest <= side {
        return dimensions;
    }
    dimensions.map(|axis| (axis * side / longest).max(assets::TILE_SIZE))
}

/// Shrinks retained terrain pixels to a planned tile without changing their layer identity.
pub(in super::super) fn reduce_texture(texture: &mut DecodedTexture, side: u32) {
    let [width, height] = reduced_dimensions([texture.width, texture.height], side);
    if [width, height] == [texture.width, texture.height] {
        return;
    }
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let source_x = x * texture.width / width;
            let source_y = y * texture.height / height;
            let offset = ((source_y * texture.width + source_x) * 4) as usize;
            pixels.extend_from_slice(&texture.rgba8[offset..offset + 4]);
        }
    }
    texture.width = width;
    texture.height = height;
    texture.rgba8 = pixels.into_boxed_slice();
}

/// Cuts strips that cannot remain terrain rectangles, then admits minimum atlas dimensions.
pub(in super::super) fn admit_static_rectangle(texture: &mut DecodedTexture) {
    if texture.height > texture.width
        || texture.width > assets::texture::policy::device_policy().max_side()
    {
        texture.retain_first_strip_frame();
    }
    let width = texture.width.max(assets::TILE_SIZE);
    let height = texture.height.max(assets::TILE_SIZE);
    if [width, height] != [texture.width, texture.height] {
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in 0..height {
            for x in 0..width {
                let source_x = x * texture.width / width;
                let source_y = y * texture.height / height;
                let offset = ((source_y * texture.width + source_x) * 4) as usize;
                pixels.extend_from_slice(&texture.rgba8[offset..offset + 4]);
            }
        }
        texture.width = width;
        texture.height = height;
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
            let (rgba8, [width, height]) = if level == 0 && bias == 0 {
                (texture.rgba8.clone(), dimensions)
            } else {
                assets::legacy_terrain_mip(&texture.rgba8, dimensions, level + bias).ok()?
            };
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
