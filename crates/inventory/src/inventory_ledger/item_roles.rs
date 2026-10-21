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
pub(super) fn loom_slot(fact: &ItemFact, banner: bool) -> Option<u8> {
    if fact.roles.contains(ContainerRoles::LOOM_PATTERN) {
        Some(11)
    } else if banner {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facts_migration_cartography_exception_requires_missing_retail_map_identities() {
        let facts = assets::facts::ItemFacts::builtin().unwrap();
        for identifier in ["minecraft:filled_map", "minecraft:map"] {
            assert!(
                facts.by_identifier(identifier).is_none(),
                "retire the cartography exception for {identifier}"
            );
            assert_eq!(cartography_slot(identifier), 12);
        }
        for record in &facts.payload().records {
            assert_eq!(
                cartography_slot(&record.identifier),
                13,
                "{}",
                record.identifier
            );
        }
        assert_eq!(cartography_slot("filled_map"), 12);
        assert_eq!(cartography_slot("map"), 12);
        assert_eq!(cartography_slot("fixture:filled_map"), 13);
    }
}
