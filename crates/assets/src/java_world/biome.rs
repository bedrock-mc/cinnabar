//! Samples biome colours from the authenticated local visual resources.

use super::{JavaBiome, JavaTexture, JavaWorldAssets};

pub const BIRCH_COLOUR: u32 = 0x80_a755;
pub const EVERGREEN_COLOUR: u32 = 0x61_9961;

/// Gamma-space colour inputs before spatial biome blending.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JavaBiomeColours {
    pub grass: u32,
    pub foliage: u32,
    pub dry_foliage: u32,
    pub water: u32,
    pub swamp_grass: bool,
}

impl JavaWorldAssets {
    /// Samples an identified counterpart; unknown custom biomes retain their native presentation.
    pub fn biome_colours(&self, identifier: &str) -> Option<JavaBiomeColours> {
        let biome = self.biomes.get(identifier)?;
        let map = |name| {
            self.textures
                .get(format!("assets/minecraft/textures/colormap/{name}.png").as_str())
        };
        let mut grass = biome.grass.or_else(|| sample(map("grass")?, biome))?;
        if biome.grass_modifier.as_deref() == Some("dark_forest") {
            grass = ((grass & 0xfe_fefe) + 0x28_340a) >> 1;
        }
        Some(JavaBiomeColours {
            grass,
            foliage: biome.foliage.or_else(|| sample(map("foliage")?, biome))?,
            dry_foliage: biome
                .dry_foliage
                .or_else(|| sample(map("dry_foliage")?, biome))?,
            water: biome.water,
            swamp_grass: biome.grass_modifier.as_deref() == Some("swamp"),
        })
    }
}

/// Maps clamped climate values to the encoded 256-square colour map without filtering.
fn sample(texture: &JavaTexture, biome: &JavaBiome) -> Option<u32> {
    if texture.width != 256 || texture.height != 256 || texture.rgba.len() != 256 * 256 * 4 {
        return None;
    }
    let temperature = f64::from(biome.temperature.clamp(0.0, 1.0));
    let humidity = f64::from(biome.downfall.clamp(0.0, 1.0)) * temperature;
    let x = ((1.0 - temperature) * 255.0) as usize;
    let y = ((1.0 - humidity) * 255.0) as usize;
    let offset = (y * 256 + x) * 4;
    let rgb = &texture.rgba[offset..offset + 3];
    Some(u32::from(rgb[0]) << 16 | u32::from(rgb[1]) << 8 | u32::from(rgb[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn climate_is_clamped_and_rainfall_is_scaled_by_temperature() {
        let pixels = (0..256)
            .flat_map(|y| (0..256).flat_map(move |x| [x as u8, y as u8, 7, 255]))
            .collect::<Vec<_>>();
        let texture = JavaTexture {
            width: 256,
            height: 256,
            rgba: pixels.into_boxed_slice(),
        };
        let biome = JavaBiome {
            environment: Default::default(),
            temperature: 0.5,
            downfall: 0.5,
            water: 0,
            grass: None,
            foliage: None,
            dry_foliage: None,
            grass_modifier: None,
            precipitation: true,
        };
        assert_eq!(sample(&texture, &biome), Some(0x7f_bf07));
        assert_eq!(
            sample(
                &texture,
                &JavaBiome {
                    temperature: 2.0,
                    downfall: -1.0,
                    ..biome
                }
            ),
            Some(0x00_ff07)
        );
    }
}
