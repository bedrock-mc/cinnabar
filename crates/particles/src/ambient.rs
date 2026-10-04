//! Native ambient leaf sampling: the sampler budget, its random stream and leaf admission.

use assets::BlockFlags;

mod random;
mod sampler;
#[cfg(test)]
mod tests;

pub use random::AmbientRandom;
pub use sampler::{SamplePlan, Sampler};

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
