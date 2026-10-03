//! Provisional item classification that steers shift-click into the slot a
//! screen would accept. A wrong guess only costs a server rejection; the
//! tables need vanilla evidence before they close a parity gate.

pub(super) fn bare(identifier: &str) -> &str {
    identifier.strip_prefix("minecraft:").unwrap_or(identifier)
}

/// Trunks smelt to charcoal, so they are ingredients before fuel.
fn is_wood_block(name: &str) -> bool {
    ends_with_any(name, &["_log", "_wood", "_stem", "_hyphae"])
}

fn ends_with_any(name: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|suffix| name.ends_with(suffix))
}

fn ends_with(name: &str, suffix: &str) -> bool {
    name.ends_with(suffix)
}

/// Whether a furnace burns the item.
#[must_use]
pub(super) fn is_fuel(identifier: &str) -> bool {
    let name = bare(identifier);
    matches!(
        name,
        "coal"
            | "charcoal"
            | "coal_block"
            | "lava_bucket"
            | "blaze_rod"
            | "dried_kelp_block"
            | "bamboo"
            | "stick"
            | "bowl"
            | "bookshelf"
            | "chest"
            | "trapped_chest"
            | "crafting_table"
            | "ladder"
            | "barrel"
            | "lectern"
            | "jukebox"
            | "note_block"
            | "composter"
            | "daylight_detector"
            | "fishing_rod"
            | "bow"
            | "crossbow"
    ) || ends_with_any(
        name,
        &[
            "_planks",
            "_log",
            "_wood",
            "_stem",
            "_hyphae",
            "_slab",
            "_stairs",
            "_fence",
            "_fence_gate",
            "_door",
            "_trapdoor",
            "_button",
            "_pressure_plate",
            "_sign",
            "_hanging_sign",
            "_boat",
            "_chest_boat",
            "_sapling",
            "_carpet",
            "_wool",
            "_banner",
        ],
    ) || name.starts_with("wooden_")
}

/// The furnace slot a shift-click aims at: `0` ingredient or `1` fuel.
#[must_use]
pub(super) fn furnace_slot(identifier: &str) -> u8 {
    let name = bare(identifier);
    if is_wood_block(name) || !is_fuel(name) {
        0
    } else {
        1
    }
}

/// The brewing-stand cells a shift-click aims at.
#[must_use]
pub(super) fn brewing_slots(identifier: &str) -> &'static [u8] {
    let name = bare(identifier);
    if name == "blaze_powder" {
        &[4]
    } else if ends_with(name, "potion") || name == "glass_bottle" {
        &[1, 2, 3]
    } else {
        &[0]
    }
}

/// The armor row (0 head .. 3 feet) an item equips into, if any.
#[must_use]
pub(super) fn armor_row(identifier: &str) -> Option<u8> {
    let name = bare(identifier);
    if ends_with(name, "_helmet") || name == "turtle_helmet" {
        Some(0)
    } else if ends_with(name, "_chestplate") || name == "elytra" {
        Some(1)
    } else if ends_with(name, "_leggings") {
        Some(2)
    } else if ends_with(name, "_boots") {
        Some(3)
    } else {
        None
    }
}

/// Whether the item is the enchanting table's lapis lazuli.
#[must_use]
pub(super) fn is_lapis(identifier: &str) -> bool {
    bare(identifier) == "lapis_lazuli"
}

/// Whether a beacon accepts the item as payment.
#[must_use]
pub(super) fn is_beacon_payment(identifier: &str) -> bool {
    matches!(
        bare(identifier),
        "iron_ingot" | "gold_ingot" | "emerald" | "diamond" | "netherite_ingot"
    )
}

/// The loom UI slot for a banner (9), dye (10) or pattern item (11).
#[must_use]
pub(super) fn loom_slot(identifier: &str) -> Option<u8> {
    let name = bare(identifier);
    if ends_with(name, "_banner_pattern") {
        Some(11)
    } else if ends_with(name, "_banner") {
        Some(9)
    } else if ends_with(name, "_dye") || name == "ink_sac" || name == "glow_ink_sac" {
        Some(10)
    } else {
        None
    }
}

/// The cartography UI slot: the map (12) or its modifier (13).
#[must_use]
pub(super) fn cartography_slot(identifier: &str) -> u8 {
    match bare(identifier) {
        "filled_map" | "map" => 12,
        _ => 13,
    }
}

/// The smithing UI slot: template (53), material (52) or equipment (51).
#[must_use]
pub(super) fn smithing_slot(identifier: &str) -> u8 {
    let name = bare(identifier);
    if ends_with(name, "_smithing_template") {
        53
    } else if ends_with(name, "_ingot") || name == "amethyst_shard" || name == "quartz" {
        52
    } else {
        51
    }
}

/// The horse-window cell an item equips into: `0` saddle, `1` armor.
#[must_use]
pub(super) fn horse_slot(identifier: &str) -> Option<u8> {
    let name = bare(identifier);
    if name == "saddle" {
        Some(0)
    } else if ends_with(name, "_horse_armor") || ends_with(name, "_carpet") {
        Some(1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn furnace_routes_fuel_and_ingredients() {
        assert_eq!(furnace_slot("minecraft:coal"), 1);
        assert_eq!(furnace_slot("minecraft:oak_log"), 0);
        assert_eq!(furnace_slot("minecraft:oak_planks"), 1);
        assert_eq!(furnace_slot("minecraft:raw_iron"), 0);
    }

    #[test]
    fn brewing_routes_by_role() {
        assert_eq!(brewing_slots("minecraft:blaze_powder"), &[4]);
        assert_eq!(brewing_slots("minecraft:splash_potion"), &[1, 2, 3]);
        assert_eq!(brewing_slots("minecraft:nether_wart"), &[0]);
    }

    #[test]
    fn equipment_and_loom_slots() {
        assert_eq!(armor_row("minecraft:iron_boots"), Some(3));
        assert_eq!(armor_row("minecraft:stick"), None);
        assert_eq!(loom_slot("minecraft:white_banner"), Some(9));
        assert_eq!(loom_slot("minecraft:creeper_banner_pattern"), Some(11));
        assert_eq!(smithing_slot("minecraft:netherite_ingot"), 52);
    }
}
