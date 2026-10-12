//! The other block of two-block structures: door halves, double plants, pitcher crops and bed
//! ends, as the native client's `getSecondPart` overrides report them.

use serde_json::{Map, Value};

/// Blocks that store `upper_block_bit` but keep the native default `getSecondPart`, which
/// reports no other part: a stacked pale moss carpet layer and a small dripleaf half stand alone.
const LONE_UPPER_BIT_BLOCKS: [&str; 2] = [
    "minecraft:pale_moss_carpet",
    "minecraft:small_dripleaf_block",
];

/// A pitcher crop is two blocks tall only above this growth stage.
const PITCHER_CROP: &str = "minecraft:pitcher_crop";
const PITCHER_CROP_SINGLE_BLOCK_GROWTH: i64 = 2;

/// The offset from block `name` in canonical `states` to the other block of its structure, if
/// it is part of one. Halves point at the block above or below them; a bed's foot points at its
/// head and its head at its foot.
pub(super) fn second_part_offset(name: &str, states: &Map<String, Value>) -> Option<[i32; 3]> {
    let value = |name: &str| states.get(name)?.get("value");
    if let Some(upper) = value("upper_block_bit") {
        if LONE_UPPER_BIT_BLOCKS.contains(&name)
            || (name == PITCHER_CROP
                && value("growth")?.as_i64()? <= PITCHER_CROP_SINGLE_BLOCK_GROWTH)
        {
            return None;
        }
        return Some([0, if upper.as_i64()? != 0 { -1 } else { 1 }, 0]);
    }
    let head = value("head_piece_bit")?.as_i64()? != 0;
    // A bed's direction 0..=3 faces south, west, north and east; its head lies ahead of its foot.
    let toward_head = match value("direction")?.as_u64()? {
        0 => [0, 0, 1],
        1 => [-1, 0, 0],
        2 => [0, 0, -1],
        3 => [1, 0, 0],
        _ => return None,
    };
    Some(toward_head.map(|axis| if head { -axis } else { axis }))
}
