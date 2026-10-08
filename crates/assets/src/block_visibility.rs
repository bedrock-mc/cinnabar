//! Shared native visibility facts for canonical vanilla terrain.

/// Whether vanilla terrain draws nothing for this block in normal play.
///
/// Invisible bedrock and moving blocks use the never-tessellated block shape; barriers, light
/// blocks and structure voids tessellate only into terrain layers drawn while a creative local
/// player holds that block. Neither kind is full, so neither culls a neighbour's face.
pub fn is_default_invisible_block(name: &str) -> bool {
    matches!(
        name,
        "minecraft:barrier"
            | "minecraft:structure_void"
            | "minecraft:invisible_bedrock"
            | "minecraft:moving_block"
    ) || name
        .strip_prefix("minecraft:light_block_")
        .is_some_and(|level| level.parse::<u8>().is_ok_and(|level| level <= 15))
}

