//! Biome data types for Minecraft Bedrock.
//!
//! Provides static biome definitions from minecraft-data.

/// Static biome definition data.
#[derive(Debug, Clone, Copy)]
pub struct BiomeData {
    /// Unique biome ID.
    pub id: u32,
    /// String identifier with namespace (e.g., "minecraft:plains").
    pub string_id: &'static str,
    /// Display name (e.g., "Plains").
    pub name: &'static str,
    /// Biome category (e.g., "forest", "ocean", "nether").
    pub category: &'static str,
    /// Dimension (e.g., "overworld", "nether", "the_end").
    pub dimension: &'static str,
    /// Biome temperature.
    pub temperature: f32,
    /// Whether this biome has precipitation (rain/snow).
    pub has_precipitation: bool,
    /// Biome color for map display (RGB).
    pub color: u32,
}
