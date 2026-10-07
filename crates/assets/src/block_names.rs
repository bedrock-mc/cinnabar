//! Shared vanilla block identities and compatibility names.

/// Current skull block identities mapped to the legacy `SkullType` ordinal.
/// Native placed-head rendering selects its model from the backing block type.
#[must_use]
pub fn vanilla_skull_type(identifier: &str) -> Option<u8> {
    Some(match identifier {
        "minecraft:skeleton_skull" => 0,
        "minecraft:wither_skeleton_skull" => 1,
        "minecraft:zombie_head" => 2,
        "minecraft:player_head" => 3,
        "minecraft:creeper_head" => 4,
        "minecraft:dragon_head" => 5,
        "minecraft:piglin_head" => 6,
        _ => return None,
    })
}

/// The older resource-pack key for a canonical vanilla block identifier.
///
/// Look up the exact identifier first: a pack may author a modern-name entry.
/// These are shared by texture and sound routing; custom namespaces and
/// unknown blocks must not inherit an unrelated vanilla entry.
#[must_use]
pub fn legacy_resource_pack_block_alias(identifier: &str) -> Option<&'static str> {
    let name = identifier.strip_prefix("minecraft:").unwrap_or(identifier);
    match name {
        "grass_block" => Some("grass"),
        "iron_chain" => Some("chain"),
        "sea_lantern" => Some("seaLantern"),
        "dandelion" => Some("yellow_flower"),
        "poppy" | "blue_orchid" | "allium" | "azure_bluet" | "red_tulip" | "orange_tulip"
        | "white_tulip" | "pink_tulip" | "oxeye_daisy" | "cornflower" | "lily_of_the_valley" => {
            Some("red_flower")
        }
        "oak_sapling" | "spruce_sapling" | "birch_sapling" | "jungle_sapling"
        | "acacia_sapling" | "dark_oak_sapling" => Some("sapling"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::legacy_resource_pack_block_alias as alias;

    #[test]
    fn resource_pack_aliases_only_apply_to_known_vanilla_names() {
        assert_eq!(alias("minecraft:grass_block"), Some("grass"));
        assert_eq!(alias("grass_block"), Some("grass"));
        assert_eq!(alias("minecraft:sea_lantern"), Some("seaLantern"));
        assert_eq!(alias("example:grass_block"), None);
        assert_eq!(alias("minecraft:unknown_block"), None);
        assert_eq!(alias("minecraft:dirt"), None);
    }
}
