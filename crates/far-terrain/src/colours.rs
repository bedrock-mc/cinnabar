//! Average texture colours of the active pack's materials, and biome tints in sRGB.

use std::collections::HashMap;

use assets::{
    LinearBiomeTints, MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_DRY_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT,
    MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_WATER_TINT, RuntimeAssets,
};

/// Per-material average sRGB colour and tint flags, and which models read as a surface from
/// above, for one runtime asset set.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceColours {
    materials: Box<[MaterialColour]>,
    /// Indexed by model template: its upward faces cover at least half a block.
    model_tops: Box<[bool]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MaterialColour {
    rgb: [f32; 3],
    flags: u32,
}

impl SurfaceColours {
    /// Averages each material's base texture layer, weighting texels by alpha.
    #[must_use]
    pub fn new(assets: &RuntimeAssets) -> Self {
        let mut layers = HashMap::<(u32, u32), [f32; 3]>::new();
        let materials = assets
            .materials()
            .iter()
            .map(|material| {
                let key = (material.texture.page(), material.texture.layer());
                let rgb = *layers
                    .entry(key)
                    .or_insert_with(|| layer_average(assets, key.0, key.1));
                MaterialColour {
                    rgb,
                    flags: material.flags,
                }
            })
            .collect();
        let templates = assets.model_templates();
        let model_tops = (0..templates.len() as u32)
            .map(|first| covers_top(assets, first))
            .collect();
        Self {
            materials,
            model_tops,
        }
    }

    /// True when the model's raised horizontal faces cover at least half a block from above,
    /// as slabs, stairs and snow layers do and torches, fences and flower pots do not.
    #[must_use]
    pub fn model_covers_top(&self, template: u32) -> bool {
        self.model_tops
            .get(template as usize)
            .copied()
            .unwrap_or(false)
    }

    /// Average sRGB of `material` tinted by `tints`, in 0..=1; unknown materials are grey.
    #[must_use]
    pub fn tinted(&self, material: u32, tints: Option<&LinearBiomeTints>) -> [f32; 3] {
        let Some(colour) = self.materials.get(material as usize) else {
            return crate::config::UNKNOWN_SURFACE_RGB.map(|value| f32::from(value) / 255.0);
        };
        let Some(tint) = tints.and_then(|tints| tint_for(colour.flags, tints)) else {
            return colour.rgb;
        };
        std::array::from_fn(|channel| colour.rgb[channel] * linear_to_srgb(tint[channel]))
    }

    /// True when `material` is tinted as water.
    #[must_use]
    pub fn is_water(&self, material: u32) -> bool {
        self.materials.get(material as usize).is_some_and(|colour| {
            colour.flags & MATERIAL_FLAG_TINT_MASK == MATERIAL_FLAG_WATER_TINT
        })
    }
}

/// Full block faces in 1/256-block squared units.
const BLOCK_AREA: f32 = 256.0 * 256.0;

/// Sums the horizontal area of every raised flat face across a compound model's parts.
fn covers_top(assets: &RuntimeAssets, first: u32) -> bool {
    let Some(parts) = assets::model_template_parts(assets.model_templates(), first) else {
        return false;
    };
    let quads = assets.model_quads();
    let area: f32 = parts
        .iter()
        .flat_map(|part| {
            let start = part.quad_start as usize;
            quads
                .get(start..start + part.quad_count as usize)
                .unwrap_or(&[])
        })
        .filter(|quad| {
            let y = quad.positions[0][1];
            y > 0 && quad.positions.iter().all(|corner| corner[1] == y)
        })
        .map(|quad| {
            let p = quad.positions.map(|[x, _, z]| (f32::from(x), f32::from(z)));
            let twice: f32 = (0..4)
                .map(|i| {
                    let (a, b) = (p[i], p[(i + 1) % 4]);
                    a.0 * b.1 - b.0 * a.1
                })
                .sum();
            twice.abs() / 2.0
        })
        .sum();
    area >= BLOCK_AREA / 2.0
}

/// The biome colour the terrain shader multiplies into a material with these flags.
fn tint_for(flags: u32, tints: &LinearBiomeTints) -> Option<[f32; 4]> {
    match flags & MATERIAL_FLAG_TINT_MASK {
        MATERIAL_FLAG_GRASS_TINT => Some(tints.grass),
        MATERIAL_FLAG_WATER_TINT => Some(tints.water),
        MATERIAL_FLAG_FOLIAGE_TINT => Some(match flags & MATERIAL_FLAG_FOLIAGE_CLASS_MASK {
            MATERIAL_FLAG_BIRCH_FOLIAGE => tints.birch,
            MATERIAL_FLAG_EVERGREEN_FOLIAGE => tints.evergreen,
            MATERIAL_FLAG_DRY_FOLIAGE => tints.dry_foliage,
            _ => tints.foliage,
        }),
        _ => None,
    }
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

/// Alpha-weighted mean of one layer's base mip, in sRGB 0..=1.
fn layer_average(assets: &RuntimeAssets, page: u32, layer: u32) -> [f32; 3] {
    let grey = crate::config::UNKNOWN_SURFACE_RGB.map(|value| f32::from(value) / 255.0);
    let Some(mip) = assets
        .texture_pages()
        .get(page as usize)
        .and_then(|page| page.texture.mips.first())
    else {
        return grey;
    };
    let layer_bytes = (mip.size as usize).pow(2) * 4;
    let start = layer as usize * layer_bytes;
    let Some(texels) = mip.rgba8.get(start..start + layer_bytes) else {
        return grey;
    };
    let mut sum = [0_u64; 3];
    let mut weight = 0_u64;
    for texel in texels.chunks_exact(4) {
        let alpha = u64::from(texel[3]);
        for channel in 0..3 {
            sum[channel] += u64::from(texel[channel]) * alpha;
        }
        weight += alpha;
    }
    if weight == 0 {
        return grey;
    }
    sum.map(|channel| channel as f32 / (weight as f32 * 255.0))
}
