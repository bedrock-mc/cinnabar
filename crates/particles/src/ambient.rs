//! Native ambient sampling, its shared random stream, and leaf/fire admission.

use assets::BlockFlags;

mod fire;
mod random;
mod sampler;
#[cfg(test)]
mod tests;

pub use fire::{FIRE_SMOKE_EFFECT, emit_fire_smoke};
pub use random::AmbientRandom;
pub use sampler::{MIN_SAMPLES, SamplePlan, Sampler};

pub const LEAF_EFFECT: &str = "minecraft:biome_tinted_leaves_particle";
pub const LEAF_CHANCE_DENOMINATOR: u32 = 100;

/// Material::_setupMaterials: air(0) and plant(8)
/// are neither solid nor liquid. TallGrass, Flower,
/// Mushroom use plant; do not infer this from collision/Cross shape.
#[must_use]
pub fn material_allows_leaf(flags: BlockFlags, identifier: Option<&str>) -> bool {
    flags.contains(BlockFlags::AIR)
        || matches!(
            identifier,
            Some(
                "minecraft:short_grass"
                    | "minecraft:fern"
                    | "minecraft:poppy"
                    | "minecraft:dandelion"
                    | "minecraft:brown_mushroom"
                    | "minecraft:red_mushroom"
            )
        )
}
