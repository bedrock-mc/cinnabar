//! Shift-click roles from pinned item facts and screen-specific defaults.
//! Routes that still rely on name guesses remain provisional.

use assets::item::armour::{ItemArmour, ItemArmourSlot, builtin_item_armour};

pub(super) fn bare(identifier: &str) -> &str {
    identifier.strip_prefix("minecraft:").unwrap_or(identifier)
}

fn ends_with(name: &str, suffix: &str) -> bool {
    name.ends_with(suffix)
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
    match builtin_item_armour(identifier) {
        ItemArmour::Wearable { slot, .. } => Some(match slot {
            ItemArmourSlot::Head => 0,
            ItemArmourSlot::Chest => 1,
            ItemArmourSlot::Legs => 2,
            ItemArmourSlot::Feet => 3,
        }),
        _ if assets::vanilla_skull_type(identifier).is_some()
            || identifier == "minecraft:carved_pumpkin" =>
        {
            Some(0)
        }
        _ => None,
    }
}

/// Whether the pinned item tags admit this exact item identity.
fn tagged(identifier: &str, tag: &str) -> bool {
    protocol::vanilla_tag_contains(tag, identifier) == Some(true)
}

/// Whether a stock item without declared components may be held in the offhand.
#[must_use]
pub(super) fn stock_offhand(identifier: &str) -> bool {
    identifier == super::TOTEM_IDENTIFIER
        || matches!(
            bare(identifier),
            "shield" | "arrow" | "firework_rocket" | "empty_map" | "filled_map" | "nautilus_shell"
        )
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
    } else if identifier == "minecraft:banner" {
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

/// The smithing UI slot admitted by item tags, or no slot for an unrelated item.
#[must_use]
pub(super) fn smithing_slot(identifier: &str) -> Option<u8> {
    if tagged(identifier, "minecraft:trim_templates")
        || tagged(identifier, "minecraft:transform_templates")
    {
        Some(53)
    } else if tagged(identifier, "minecraft:trim_materials")
        || tagged(identifier, "minecraft:transform_materials")
    {
        Some(52)
    } else if tagged(identifier, "minecraft:trimmable_armors")
        || tagged(identifier, "minecraft:transformable_items")
    {
        Some(51)
    } else {
        None
    }
}

/// The horse-window cell an item equips into: `0` saddle, `1` armor.
#[must_use]
pub(super) fn horse_slot(identifier: &str) -> Option<u8> {
    let name = bare(identifier);
    if name == "saddle" {
        Some(0)
    } else if tagged(identifier, "minecraft:horse_armor") || ends_with(name, "_carpet") {
        Some(1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(armor_row("minecraft:carved_pumpkin"), Some(0));
        assert_eq!(armor_row("minecraft:elytra"), Some(1));
        assert!(stock_offhand("minecraft:arrow"));
        assert!(!stock_offhand("minecraft:dirt"));
        assert_eq!(loom_slot("minecraft:banner"), Some(9));
        assert_eq!(loom_slot("minecraft:creeper_banner_pattern"), Some(11));
        assert_eq!(smithing_slot("minecraft:netherite_ingot"), Some(52));
    }
}
