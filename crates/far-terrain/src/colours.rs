//! Average texture colours of the active pack's materials, and biome tints in sRGB.

use std::collections::HashMap;

use assets::{
    LinearBiomeTints, MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_DRY_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT,
    MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_WATER_TINT, RuntimeAssets,
};

/// Per-material average sRGB colour and tint flags for one runtime asset set.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceColours {
    materials: Box<[MaterialColour]>,
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
        Self { materials }
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
