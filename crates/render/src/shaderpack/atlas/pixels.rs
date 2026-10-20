//! CPU assembly of atlas and frame-store mips from carrier pages and authored maps.

use super::labpbr::{NEUTRAL_NORMAL, NEUTRAL_SPECULAR, labpbr_normal, labpbr_specular};
use super::pack::{PackedAtlas, SpriteRect};
use super::plan::{
    AtlasInputs, AtlasPlan, AuthoredPbr, LayerLayout, MISSING_SPRITE_SIDE, SpriteContent,
};
use assets::TextureArray;

/// Missing-sprite colours, alternating in quadrants.
const MISSING_COLOURS: [[u8; 4]; 2] = [[255, 0, 255, 255], [0, 0, 0, 255]];

/// Tightly packed RGBA8 levels; level `m` is `(width >> m) × (height >> m)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MipImages {
    pub width: u32,
    pub height: u32,
    pub levels: Vec<Vec<u8>>,
}

impl MipImages {
    /// Levels filled with one texel, the value unoccupied texels keep.
    pub fn filled(width: u32, height: u32, levels: u32, texel: [u8; 4]) -> Self {
        Self {
            width,
            height,
            levels: (0..levels)
                .map(|level| texel.repeat(((width >> level) * (height >> level)) as usize))
                .collect(),
        }
    }

    /// Copies `texels` (a `rect.side >> level` square) into `rect` at `level`.
    pub fn blit(&mut self, level: u32, rect: SpriteRect, texels: &[u8]) {
        let rect = rect.at_mip(level);
        let stride = (self.width >> level) as usize * 4;
        let row = rect.side as usize * 4;
        let image = &mut self.levels[level as usize];
        for (y, source) in texels.chunks_exact(row).enumerate() {
            let start = (rect.y as usize + y) * stride + rect.x as usize * 4;
            image[start..start + row].copy_from_slice(source);
        }
    }

    /// The `rect` square at `level`, row-major.
    #[cfg(test)]
    pub fn read(&self, level: u32, rect: SpriteRect) -> Vec<u8> {
        let rect = rect.at_mip(level);
        let stride = (self.width >> level) as usize * 4;
        let row = rect.side as usize * 4;
        let image = &self.levels[level as usize];
        (0..rect.side as usize)
            .flat_map(|y| {
                let start = (rect.y as usize + y) * stride + rect.x as usize * 4;
                image[start..start + row].iter().copied()
            })
            .collect()
    }
}

/// Colour, normal and specular levels sharing one layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LayerImages {
    pub color: MipImages,
    /// `None` keeps the neutral LabPBR values throughout.
    pub pbr: Option<[MipImages; 2]>,
}

/// Atlas levels plus the frame store animations copy from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AtlasImages {
    pub atlas: LayerImages,
    pub frames: Option<LayerImages>,
}

/// Extracts the sampled region at source LOD before removing physical page expansion.
/// Malformed pages or absent layers return `None`.
pub(crate) fn region_texels(
    texture: &TextureArray,
    layer: u32,
    layout: LayerLayout,
    side: u32,
) -> Option<Vec<u8>> {
    if layer >= texture.layers || side == 0 {
        return None;
    }
    let source_lod = (layout.source_side.max(1) / side)
        .checked_ilog2()
        .unwrap_or(0);
    let (mip, region) = texture
        .mips
        .iter()
        .take(source_lod as usize + 1)
        .filter_map(|mip| {
            let region = mip.size.checked_shr(u32::from(layout.grid))?;
            (region > 0).then_some((mip, region))
        })
        .next_back()?;
    let size = mip.size as usize;
    let layer_bytes = size.checked_mul(size)?.checked_mul(4)?;
    if mip.rgba8.len() != layer_bytes.checked_mul(texture.layers as usize)? {
        return None;
    }
    let source = mip.rgba8.get(layer as usize * layer_bytes..)?;
    let region = region as usize;
    let mut pixels = Vec::with_capacity(region * region * 4);
    for row in 0..region {
        pixels.extend_from_slice(source.get(row * size * 4..(row * size + region) * 4)?);
    }
    Some(resample(&pixels, region as u32, side))
}

/// Box-filters `source` (a `source_side` square) down by whole factors; other ratios repeat
/// the nearest texel.
pub(crate) fn resample(source: &[u8], source_side: u32, side: u32) -> Vec<u8> {
    if source_side == side {
        return source.to_vec();
    }
    let (source_side, side) = (source_side as usize, side as usize);
    let mut pixels = Vec::with_capacity(side * side * 4);
    if source_side > side && source_side.is_multiple_of(side) {
        let factor = source_side / side;
        let count = (factor * factor) as u32;
        for y in 0..side {
            for x in 0..side {
                let mut sum = [0_u32; 4];
                for sy in y * factor..(y + 1) * factor {
                    for sx in x * factor..(x + 1) * factor {
                        let at = (sy * source_side + sx) * 4;
                        for (total, &value) in sum.iter_mut().zip(&source[at..at + 4]) {
                            *total += u32::from(value);
                        }
                    }
                }
                pixels.extend(sum.map(|total| ((total + count / 2) / count) as u8));
            }
        }
    } else {
        for y in 0..side {
            for x in 0..side {
                let at = (y * source_side / side * source_side + x * source_side / side) * 4;
                pixels.extend_from_slice(&source[at..at + 4]);
            }
        }
    }
    pixels
}

/// The missing sprite's quadrant checker at `side`².
pub(crate) fn missing_texels(side: u32) -> Vec<u8> {
    let half = MISSING_SPRITE_SIDE / 2;
    let base = (0..MISSING_SPRITE_SIDE)
        .flat_map(|y| {
            (0..MISSING_SPRITE_SIDE)
                .flat_map(move |x| MISSING_COLOURS[usize::from((x < half) != (y < half))])
        })
        .collect::<Vec<_>>();
    resample(&base, MISSING_SPRITE_SIDE, side)
}

/// Colour texels of a sprite at `side`²; absent or malformed layers show the missing sprite.
pub(crate) fn color_texels(inputs: &AtlasInputs<'_>, content: SpriteContent, side: u32) -> Vec<u8> {
    let SpriteContent::Layer { reference, opaque } = content else {
        return missing_texels(side);
    };
    let Some(mut texels) = inputs
        .layer_page(reference)
        .and_then(|page| region_texels(page, reference.layer(), (inputs.layout)(reference), side))
    else {
        return missing_texels(side);
    };
    if opaque {
        for texel in texels.as_chunks_mut::<4>().0 {
            texel[3] = u8::MAX;
        }
    }
    texels
}

/// LabPBR normal and specular texels at `side`² from the resolved authored layer;
/// unauthored, conflicting or malformed entries stay neutral.
pub(crate) fn pbr_texels(
    authored: &AuthoredPbr<'_>,
    content: SpriteContent,
    side: u32,
) -> [Vec<u8>; 2] {
    let neutral = || {
        [
            NEUTRAL_NORMAL.repeat((side * side) as usize),
            NEUTRAL_SPECULAR.repeat((side * side) as usize),
        ]
    };
    let SpriteContent::Layer { reference, .. } = content else {
        return neutral();
    };
    let slot = reference.page() as usize * (assets::TEXTURE_LAYER_MASK as usize + 1)
        + reference.layer() as usize;
    let Some(&authored_ref) = authored.refs.get(slot).filter(|&&value| value != u32::MAX) else {
        return neutral();
    };
    let page = (authored_ref >> 31) as usize;
    let layer = authored_ref & (assets::texture::PBR_REF_COLOR - 1);
    let convert =
        |pages: [&TextureArray; 2], map: fn([u8; 4], u32) -> [u8; 4], fallback: [u8; 4]| {
            region_texels(
                pages[page],
                layer,
                LayerLayout {
                    grid: 0,
                    source_side: pages[page].mips.first().map_or(0, |mip| mip.size),
                },
                side,
            )
            .map_or_else(
                || fallback.repeat((side * side) as usize),
                |texels| {
                    texels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|&texel| map(texel, authored_ref))
                        .collect()
                },
            )
        };
    [
        convert(authored.normal_pages, labpbr_normal, NEUTRAL_NORMAL),
        convert(authored.mer_pages, labpbr_specular, NEUTRAL_SPECULAR),
    ]
}

/// Renders every sprite of `packed` at each of `levels` mips.
fn layer_images(
    inputs: &AtlasInputs<'_>,
    packed: &PackedAtlas,
    contents: &[SpriteContent],
    levels: u32,
) -> LayerImages {
    let (width, height) = (packed.width, packed.height);
    let mut color = MipImages::filled(width, height, levels, [0; 4]);
    let mut pbr = inputs.authored.as_ref().map(|_| {
        [
            MipImages::filled(width, height, levels, NEUTRAL_NORMAL),
            MipImages::filled(width, height, levels, NEUTRAL_SPECULAR),
        ]
    });
    for (&rect, &content) in packed.rects.iter().zip(contents) {
        for level in 0..levels {
            let side = rect.side >> level;
            color.blit(level, rect, &color_texels(inputs, content, side));
            if let (Some(authored), Some([normals, specular])) = (&inputs.authored, pbr.as_mut()) {
                let [normal_texels, specular_texels] = pbr_texels(authored, content, side);
                normals.blit(level, rect, &normal_texels);
                specular.blit(level, rect, &specular_texels);
            }
        }
    }
    LayerImages { color, pbr }
}

/// Builds the atlas levels (animated slots show their first frame) and the frame store.
pub(crate) fn build_images(inputs: &AtlasInputs<'_>, plan: &AtlasPlan) -> AtlasImages {
    AtlasImages {
        atlas: layer_images(inputs, &plan.atlas, &plan.contents, plan.mip_levels),
        frames: plan
            .frames
            .as_ref()
            .map(|frames| layer_images(inputs, frames, &plan.frame_contents, plan.mip_levels)),
    }
}
