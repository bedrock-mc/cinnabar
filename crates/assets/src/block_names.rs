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
        "trip_wire" => Some("tripWire"),
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
    fn registered_trip_wire_resolves_the_legacy_pack_entry() {
        let registry = crate::read_registry_for_protocol(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .unwrap();
        let record = registry
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:trip_wire")
            .unwrap();
        let key = alias(&record.name).unwrap();
        assert_eq!(key, "tripWire");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = root
            .join(crate::vanilla_source().resource_pack_dir())
            .join("blocks.json");
        if !path.is_file() {
            eprintln!("skipping trip-wire pack-entry check: missing pinned blocks.json");
            return;
        }
        let blocks: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(blocks[key]["textures"], "trip_wire");
        assert_eq!(alias("example:trip_wire"), None);
    }

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
