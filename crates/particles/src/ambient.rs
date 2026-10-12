//! Native ambient sampling, its shared random stream, and leaf/fire admission.

use assets::native_material::NativeMaterialFacts;

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

/// Reads the admitted solid/liquid disposition without inspecting block names or geometry.
#[must_use]
pub const fn material_allows_leaf(material: NativeMaterialFacts) -> bool {
    matches!(
        material,
        NativeMaterialFacts::NonSolidNonLiquid | NativeMaterialFacts::ProvisionalNonSolidNonLiquid
    )
}
