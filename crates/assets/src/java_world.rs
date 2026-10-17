//! Local runtime resources for the optional Java world presentation.

mod archive;
mod attributes;
mod biome;
mod mips;
mod textures;
mod timeline;
mod water;

pub use archive::{JavaWorldSource, compile_java_world, decode_java_world};
pub use attributes::{Attribute, AttributeValue, JavaEnvironment};
pub use biome::{BIRCH_COLOUR, EVERGREEN_COLOUR, JavaBiomeColours};
pub use mips::{mean_mip, terrain_layer};
use std::collections::BTreeMap;
pub use water::{JavaWaterTexture, WATER_TEXTURE_PATHS};

pub const SOURCE_MANIFEST: &str = include_str!("../../../assets/java-world-source.json");
pub const CARRIER_FILENAME: &str = "java-world-v1.mcjwv";
pub const MAX_CARRIER_BYTES: usize = 8 * 1024 * 1024;

/// Decoded resources shared by environment evaluation and graphics upload.
#[derive(Clone, Debug)]
pub struct JavaWorldAssets {
    pub source: JavaWorldSource,
    pub textures: BTreeMap<Box<str>, JavaTexture>,
    pub water: [JavaWaterTexture; WATER_TEXTURE_PATHS.len()],
    pub biomes: BTreeMap<Box<str>, JavaBiome>,
    atmosphere: [crate::AtmosphereTexture; crate::AtmosphereRole::ALL.len()],
    dimensions: [JavaEnvironment; 3],
    timelines: Vec<timeline::Timeline>,
}

/// One bounded visual texture without any graphics-backend ownership.
#[derive(Clone, Debug)]
pub struct JavaTexture {
    pub width: u32,
    pub height: u32,
    pub rgba: Box<[u8]>,
}

/// Colour inputs for a built-in biome with a Java counterpart.
#[derive(Clone, Debug)]
pub struct JavaBiome {
    pub environment: JavaEnvironment,
    pub temperature: f32,
    pub downfall: f32,
    pub water: u32,
    pub grass: Option<u32>,
    pub foliage: Option<u32>,
    pub dry_foliage: Option<u32>,
    pub grass_modifier: Option<Box<str>>,
    pub precipitation: bool,
}

impl JavaWorldAssets {
    /// Borrows a prepared sky image without per-frame conversion.
    pub fn atmosphere_texture(&self, role: crate::AtmosphereRole) -> &crate::AtmosphereTexture {
        &self.atmosphere[role as usize - 1]
    }

    /// Samples immutable dimension, biome and day inputs without per-frame allocation.
    pub fn environment(&self, dimension: i32, biome: Option<&str>, ticks: f64) -> JavaEnvironment {
        let dimension = usize::try_from(dimension)
            .ok()
            .filter(|index| *index < 3)
            .unwrap_or(0);
        let mut values = self.dimensions[dimension];
        if let Some(biome) = biome.and_then(|name| self.biomes.get(name)) {
            values.overlay(&biome.environment);
        }
        if dimension == 0 {
            for timeline in &self.timelines {
                timeline.apply(ticks, &mut values);
            }
        }
        values
    }
}
