//! Iris's biome uniforms (`biome`, `biome_category`, `biome_precipitation`, `temperature`,
//! `rainfall`) and the `BIOME_*`, `CAT_*` and `PPT_*` macros its `IrisDefines` injects into
//! every program and `shaders.properties`, over the pinned Java biome registry of
//! [`JavaBlockMapping`].
//!
//! Iris numbers biomes in `Biomes.register` order; the pinned registry is in Java's network
//! registry order instead, so `biome` and `BIOME_*` values differ from Iris's numerically
//! while every comparison between them (the only way packs use them) agrees.

use assets::facts::java_mapping::{JavaBiomeEntry, JavaBlockMapping};

/// Declares biome categories and macro names together in ordinal order.
macro_rules! biome_categories {
    ($($category:ident => $name:literal),+ $(,)?) => {
        /// Iris's biome categories, in ordinal order.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum BiomeCategory { $($category),+ }

        impl BiomeCategory {
            /// Every category in ordinal order.
            pub const ALL: [Self; [$(stringify!($category)),+].len()] = [$(Self::$category),+];

            /// The `CAT_*` macro naming this category.
            pub const fn macro_name(self) -> &'static str {
                match self { $(Self::$category => $name),+ }
            }
        }
    };
}

biome_categories! {
    None => "CAT_NONE",
    Taiga => "CAT_TAIGA",
    ExtremeHills => "CAT_EXTREME_HILLS",
    Jungle => "CAT_JUNGLE",
    Mesa => "CAT_MESA",
    Plains => "CAT_PLAINS",
    Savanna => "CAT_SAVANNA",
    Icy => "CAT_ICY",
    TheEnd => "CAT_THE_END",
    Beach => "CAT_BEACH",
    Forest => "CAT_FOREST",
    Ocean => "CAT_OCEAN",
    Desert => "CAT_DESERT",
    River => "CAT_RIVER",
    Swamp => "CAT_SWAMP",
    Mushroom => "CAT_MUSHROOM",
    Nether => "CAT_NETHER",
    Mountain => "CAT_MOUNTAIN",
    Underground => "CAT_UNDERGROUND",
}

impl BiomeCategory {
    /// The `biome_category` value.
    pub const fn ordinal(self) -> i32 {
        self as i32
    }
}

/// Iris's `BiomeUniforms.getBiomeCategory` for current Java versions: the category of the
/// first biome tag the biome carries, else plains. (Iris has no rule for `UNDERGROUND` since
/// Java dropped the tag it used.)
const CATEGORY_RULES: &[(&str, BiomeCategory)] = &[
    (
        "minecraft:without_wandering_trader_spawns",
        BiomeCategory::None,
    ),
    ("minecraft:has_structure/village_snowy", BiomeCategory::Icy),
    ("minecraft:is_hill", BiomeCategory::ExtremeHills),
    ("minecraft:is_taiga", BiomeCategory::Taiga),
    ("minecraft:is_ocean", BiomeCategory::Ocean),
    ("minecraft:is_jungle", BiomeCategory::Jungle),
    ("minecraft:is_forest", BiomeCategory::Forest),
    ("minecraft:is_badlands", BiomeCategory::Mesa),
    ("minecraft:is_nether", BiomeCategory::Nether),
    ("minecraft:is_end", BiomeCategory::TheEnd),
    ("minecraft:is_beach", BiomeCategory::Beach),
    (
        "minecraft:has_structure/desert_pyramid",
        BiomeCategory::Desert,
    ),
    ("minecraft:is_river", BiomeCategory::River),
    (
        "minecraft:allows_surface_slime_spawns",
        BiomeCategory::Swamp,
    ),
    ("minecraft:without_zombie_sieges", BiomeCategory::Mushroom),
    ("minecraft:is_mountain", BiomeCategory::Mountain),
];

/// Iris's precipitation kinds: `biome_precipitation` and the `PPT_*` macros.
const PRECIPITATION_MACROS: [&str; 3] = ["PPT_NONE", "PPT_RAIN", "PPT_SNOW"];

/// What Java reads unmapped biomes as: Iris's lookup of a biome it never numbered falls back
/// to its first id, the void.
const FALLBACK_BIOME: &str = "minecraft:the_void";

/// Java's overworld sea level; the snow line lapse starts 17 blocks above it.
const SEA_LEVEL: i32 = 63;
const LAPSE_START_ABOVE_SEA: i32 = 17;
/// Java's temperature loss per block above the lapse start (`0.05 / 40`).
const LAPSE_PER_BLOCK: f32 = 0.05 / 40.0;
/// Below this height-adjusted temperature precipitation falls as snow.
const SNOW_BELOW: f32 = 0.15;

/// The biome uniforms at one block, as Iris reads them for the player's block position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BiomeUniforms {
    /// `biome`: the Java biome's registry index (compare with `BIOME_*`).
    pub biome: i32,
    /// `biome_category`: a [`BiomeCategory`] ordinal (compare with `CAT_*`).
    pub category: i32,
    /// `biome_precipitation`: 0 none, 1 rain, 2 snow.
    pub precipitation: i32,
    /// `temperature`: the Java biome's base temperature.
    pub temperature: f32,
    /// `rainfall`: the Java biome's downfall.
    pub rainfall: f32,
}

impl BiomeUniforms {
    /// Iris's values without a player: every uniform 0.
    pub const NONE: BiomeUniforms = BiomeUniforms {
        biome: 0,
        category: 0,
        precipitation: 0,
        temperature: 0.0,
        rainfall: 0.0,
    };

    /// The uniforms for the Bedrock biome `bedrock_biome` at block height `block_y`, through
    /// its Java biome; a Bedrock biome without one reads as the void. `None` when the
    /// mapping has no Java biome registry.
    pub fn at(
        mapping: &JavaBlockMapping,
        bedrock_biome: u32,
        block_y: i32,
    ) -> Option<BiomeUniforms> {
        let index = mapping
            .java_biome_index(bedrock_biome)
            .or_else(|| fallback_biome_index(mapping))?;
        let entry = mapping.java_biome_entry(index)?;
        Some(BiomeUniforms {
            biome: i32::from(index),
            category: category(mapping, index).ordinal(),
            precipitation: precipitation(&entry, block_y),
            temperature: entry.temperature,
            rainfall: entry.downfall,
        })
    }
}

/// The registry index of [`FALLBACK_BIOME`], what Java reads unmapped biomes as.
pub(crate) fn fallback_biome_index(mapping: &JavaBlockMapping) -> Option<u16> {
    mapping
        .java_biomes()
        .position(|biome| biome.name == FALLBACK_BIOME)
        .and_then(|index| u16::try_from(index).ok())
}

/// The Iris category of the Java biome at `index`.
fn category(mapping: &JavaBlockMapping, index: u16) -> BiomeCategory {
    CATEGORY_RULES
        .iter()
        .find(|(tag, _)| mapping.java_biome_has_tag(index, tag))
        .map_or(BiomeCategory::Plains, |(_, category)| *category)
}

/// Java's `Biome.getPrecipitationAt`: none without precipitation, else snow where the
/// height-adjusted temperature is below 0.15. Java's horizontal temperature noise above the
/// lapse start and the frozen-ocean modifier are not modelled.
fn precipitation(entry: &JavaBiomeEntry<'_>, block_y: i32) -> i32 {
    if !entry.has_precipitation {
        return 0;
    }
    let lapse_start = SEA_LEVEL + LAPSE_START_ABOVE_SEA;
    let above = (block_y - lapse_start).max(0) as f32;
    if entry.temperature - above * LAPSE_PER_BLOCK < SNOW_BELOW {
        2
    } else {
        1
    }
}

/// `BIOME_<PATH>` for every Java biome, `CAT_*` and `PPT_*`, as `IrisDefines` injects them.
pub(crate) fn biome_defines(mapping: Option<&JavaBlockMapping>) -> Vec<(String, String)> {
    let mut defines = Vec::new();
    if let Some(mapping) = mapping {
        for (index, biome) in mapping.java_biomes().enumerate() {
            let path = biome.name.rsplit(':').next().unwrap_or(biome.name);
            defines.push((
                format!("BIOME_{}", path.to_ascii_uppercase()),
                index.to_string(),
            ));
        }
    }
    for category in BiomeCategory::ALL {
        defines.push((
            category.macro_name().to_owned(),
            category.ordinal().to_string(),
        ));
    }
    for (value, name) in PRECIPITATION_MACROS.iter().enumerate() {
        defines.push(((*name).to_owned(), value.to_string()));
    }
    defines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping() -> Option<&'static JavaBlockMapping> {
        let mapping = JavaBlockMapping::pinned().ok();
        if mapping.is_none() {
            eprintln!("skipping: missing Java mapping carrier fixture");
        }
        mapping
    }

    fn bedrock_id(mapping: &JavaBlockMapping, java: &str) -> u32 {
        (0..u32::from(u16::MAX))
            .find(|id| mapping.java_biome(*id) == Some(java))
            .unwrap_or_else(|| panic!("no Bedrock biome maps to {java}"))
    }

    fn define(defines: &[(String, String)], name: &str) -> i32 {
        defines
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} is not defined"))
            .1
            .parse()
            .unwrap()
    }

    #[test]
    fn categories_and_climate_follow_iris_for_representative_biomes() {
        let Some(mapping) = mapping() else { return };
        let defines = biome_defines(Some(mapping));
        for (java, biome_macro, category, precipitation, temperature, rainfall) in [
            (
                "minecraft:plains",
                "BIOME_PLAINS",
                "CAT_PLAINS",
                "PPT_RAIN",
                0.8,
                0.4,
            ),
            (
                "minecraft:desert",
                "BIOME_DESERT",
                "CAT_DESERT",
                "PPT_NONE",
                2.0,
                0.0,
            ),
            (
                "minecraft:swamp",
                "BIOME_SWAMP",
                "CAT_SWAMP",
                "PPT_RAIN",
                0.8,
                0.9,
            ),
            (
                "minecraft:snowy_plains",
                "BIOME_SNOWY_PLAINS",
                "CAT_ICY",
                "PPT_SNOW",
                0.0,
                0.5,
            ),
            (
                "minecraft:taiga",
                "BIOME_TAIGA",
                "CAT_TAIGA",
                "PPT_RAIN",
                0.25,
                0.8,
            ),
            (
                "minecraft:ocean",
                "BIOME_OCEAN",
                "CAT_OCEAN",
                "PPT_RAIN",
                0.5,
                0.5,
            ),
            (
                "minecraft:badlands",
                "BIOME_BADLANDS",
                "CAT_MESA",
                "PPT_NONE",
                2.0,
                0.0,
            ),
            (
                "minecraft:mushroom_fields",
                "BIOME_MUSHROOM_FIELDS",
                "CAT_MUSHROOM",
                "PPT_RAIN",
                0.9,
                1.0,
            ),
        ] {
            let uniforms = BiomeUniforms::at(mapping, bedrock_id(mapping, java), 64).unwrap();
            assert_eq!(uniforms.biome, define(&defines, biome_macro), "{java}");
            assert_eq!(uniforms.category, define(&defines, category), "{java}");
            assert_eq!(
                uniforms.precipitation,
                define(&defines, precipitation),
                "{java}"
            );
            assert_eq!(uniforms.temperature, temperature, "{java}");
            assert_eq!(uniforms.rainfall, rainfall, "{java}");
        }
    }

    #[test]
    fn rain_turns_to_snow_above_the_java_snow_line() {
        let Some(mapping) = mapping() else { return };
        // Plains (0.8) cools below 0.15 after (0.8 - 0.15) / (0.05 / 40) = 520 blocks.
        let plains = bedrock_id(mapping, "minecraft:plains");
        let at = |y| BiomeUniforms::at(mapping, plains, y).unwrap().precipitation;
        assert_eq!(at(80), 1);
        assert_eq!(at(80 + 519), 1);
        assert_eq!(at(80 + 521), 2);
    }

    #[test]
    fn unmapped_bedrock_biomes_read_as_the_void() {
        let Some(mapping) = mapping() else { return };
        let defines = biome_defines(Some(mapping));
        let uniforms = BiomeUniforms::at(mapping, u32::MAX, 64).unwrap();
        assert_eq!(uniforms.biome, define(&defines, "BIOME_THE_VOID"));
        assert_eq!(uniforms.category, define(&defines, "CAT_NONE"));
        assert_eq!(uniforms.precipitation, 0);
    }

    #[test]
    fn category_and_precipitation_macros_take_iris_ordinals() {
        let defines = biome_defines(None);
        assert_eq!(define(&defines, "CAT_NONE"), 0);
        assert_eq!(define(&defines, "CAT_SWAMP"), 14);
        assert_eq!(define(&defines, "CAT_UNDERGROUND"), 18);
        assert_eq!(define(&defines, "PPT_SNOW"), 2);
    }
}
