//! Shift-click roles from the admitted numeric item facts.

use assets::facts::{ContainerRoles, ItemFact};

/// The brewing-stand cells a shift-click aims at.
#[must_use]
pub(super) fn brewing_slots(fact: &ItemFact) -> &'static [u8] {
    if fact.roles.contains(ContainerRoles::BREWING_FUEL) {
        &[4]
    } else if fact.roles.contains(ContainerRoles::BREWING_BOTTLE) {
        &[1, 2, 3]
    } else {
        &[0]
    }
}

/// The loom UI slot for a banner (9), dye (10) or pattern item (11).
#[must_use]
pub(super) fn loom_slot(fact: &ItemFact) -> Option<u8> {
    if fact.roles.contains(ContainerRoles::LOOM_PATTERN) {
        Some(11)
    } else if fact.roles.contains(ContainerRoles::LOOM_BANNER) {
        Some(9)
    } else if fact.roles.contains(ContainerRoles::LOOM_DYE) {
        Some(10)
    } else {
        None
    }
}

/// The cartography UI slot: the map (12) or its modifier (13).
#[must_use]
pub(super) fn cartography_slot(identifier: &str) -> u8 {
    // These map identities are absent from the pinned retail facts carrier.
    match identifier.strip_prefix("minecraft:").unwrap_or(identifier) {
        "filled_map" | "map" => 12,
        _ => 13,
    }
}

/// The smithing UI slot admitted by item tags, or no slot for an unrelated item.
#[must_use]
pub(super) fn smithing_slot(fact: &ItemFact) -> Option<u8> {
    if fact.roles.contains(ContainerRoles::SMITHING_TEMPLATE) {
        Some(53)
    } else if fact.roles.contains(ContainerRoles::SMITHING_MATERIAL) {
        Some(52)
    } else if fact.roles.contains(ContainerRoles::SMITHING_INPUT) {
        Some(51)
    } else {
        None
    }
}

/// The horse-window cell an item equips into: `0` saddle, `1` armor or decor.
#[must_use]
pub(super) fn horse_slot(fact: &ItemFact) -> Option<u8> {
    if fact.roles.contains(ContainerRoles::HORSE_SADDLE) {
        Some(0)
    } else if fact.roles.contains(ContainerRoles::HORSE_ARMOUR)
        || fact.roles.contains(ContainerRoles::HORSE_DECOR)
    {
        Some(1)
    } else {
        None
    }
}
