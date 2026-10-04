//! Native selection bounds are independent of movement collision geometry.
//!
//! The interaction ray and outline both consume these bounds. Passable foliage
//! and the first snow layer remain selectable without a movement collider.
//! Unreviewed blocks retain their existing registry bounds.

use assets::{RegistryRecord, TOP_SNOW_LAYER_COUNT};
use sim::{Aabb, Vec3};

/// Visual bounds for plants; `BlockType::clip` picks these independently
/// of movement collision.
pub(super) fn shape(record: &RegistryRecord) -> Option<Aabb> {
    let name = record.name.strip_prefix("minecraft:")?;
    if name == "snow_layer" {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
        let height = state.get("height")?.get("value")?.as_u64()?;
        if height >= u64::from(TOP_SNOW_LAYER_COUNT) {
            return None;
        }
        // TopSnowBlock::getVisualShape: full X/Z,
        // visual/outline height (height + 1)/8, independently of collision.
        return Some(Aabb::new(
            Vec3::ZERO,
            Vec3::new(
                1.0,
                (height + 1) as f64 / f64::from(TOP_SNOW_LAYER_COUNT),
                1.0,
            ),
        ));
    }
    if matches!(
        name,
        "torch" | "soul_torch" | "redstone_torch" | "unlit_redstone_torch"
    ) {
        return torch_shape(record);
    }
    let (inset, height, edge): (f32, f32, f32) = match name {
        "short_grass" | "fern" => (0.1, 0.8, 0.9),
        "short_dry_grass" => (0.125, 0.625, 0.875),
        "brown_mushroom" | "red_mushroom" => (0.3, 0.4, 0.7),
        "nether_sprouts" => (0.15, 0.3, 0.85),
        "cactus_flower" => (0.0625, 0.875, 0.9375),
        "reeds" => (0.125, 1.0, 0.875),
        // Crops and nether wart span the cell width and are .25 blocks tall.
        "wheat" | "carrots" | "potatoes" | "beetroot" | "nether_wart" => (0.0, 0.25, 1.0),
        "dandelion" | "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "red_tulip"
        | "orange_tulip" | "white_tulip" | "pink_tulip" | "oxeye_daisy" | "cornflower"
        | "lily_of_the_valley" | "wither_rose" | "torchflower" => (0.15, 0.6, 0.7),
        "oak_sapling" | "spruce_sapling" | "birch_sapling" | "jungle_sapling"
        | "acacia_sapling" | "dark_oak_sapling" | "cherry_sapling" | "pale_oak_sapling" => {
            (0.1, 0.8, 0.9)
        }
        // DeadBushBlock constructor overrides inherited
        // flower bounds with grass-sized bounds, including maxY=.8.
        "deadbush" => (0.1, 0.8, 0.9),
        // BushBlock uses
        // minXYZ=(0,0,0), maxX=1; ctor literals set maxY=.8 and maxZ=1.
        "bush" => (0.0, 0.8, 1.0),
        "tall_grass" | "large_fern" | "sunflower" | "lilac" | "rose_bush" | "peony" => {
            let state: serde_json::Value = serde_json::from_str(&record.canonical_state).ok()?;
            let upper = state.get("upper_block_bit")?.get("value")?.as_i64()? != 0;
            (0.3, if upper { 0.6 } else { 1.0 }, 0.7)
        }
        _ => return None,
    };
    Some(bounds([inset, 0.0, inset], [edge, height, edge]))
}

/// TorchBlock chooses the visual box by `torch_facing_direction`, independently
/// of its empty collision. Four wall boxes and one upright box cover the orientations.
fn torch_shape(record: &RegistryRecord) -> Option<Aabb> {
    let state = serde_json::from_str::<serde_json::Value>(&record.canonical_state).ok()?;
    let facing = state["torch_facing_direction"]["value"].as_str()?;
    let (min, max) = match facing {
        "west" => ([0.0, 0.2, 0.35], [0.3, 0.8, 0.65]),
        "east" => ([0.7, 0.2, 0.35], [1.0, 0.8, 0.65]),
        "north" => ([0.35, 0.2, 0.0], [0.65, 0.8, 0.3]),
        "south" => ([0.35, 0.2, 0.7], [0.65, 0.8, 1.0]),
        _ => ([0.4, 0.0, 0.4], [0.6, 0.6, 0.6]),
    };
    Some(bounds(min, max))
}

/// Converts native f32 selection coordinates to the simulator bounds type.
fn bounds(min: [f32; 3], max: [f32; 3]) -> Aabb {
    let point = |[x, y, z]: [f32; 3]| Vec3::new(f64::from(x), f64::from(y), f64::from(z));
    Aabb::new(point(min), point(max))
}

#[cfg(test)]
mod tests;
