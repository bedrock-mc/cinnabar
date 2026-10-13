use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};

pub const ITEM_ARMOUR_SOURCE_PATH: &str = "registry/item-armour-v2193.json";
pub const ITEM_ARMOUR_SOURCE_BYTES: &[u8] = include_bytes!("../../data/item-armour-v2193.json");

/// Armour facts admitted for an item; unresolved data is never a zero-value claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "support", deny_unknown_fields)]
pub enum ItemArmour {
    Unresolved,
    NotArmour,
    Wearable {
        slot: ItemArmourSlot,
        protection: u32,
    },
}

/// Player armour slots, independent of numeric inventory container slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemArmourSlot {
    Head,
    Chest,
    Legs,
    Feet,
}

impl ItemArmour {
    /// Returns admitted protection, with absence of armour distinct from unresolved data.
    pub const fn protection(self) -> Option<u32> {
        match self {
            Self::Unresolved => None,
            Self::NotArmour => Some(0),
            Self::Wearable { protection, .. } => Some(protection),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: u32,
    protocol: u32,
    items: Box<[Entry]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    identifier: Box<str>,
    armour: ItemArmour,
}

/// Returns pinned armour facts for an exact item identity; missing entries stay unresolved.
#[must_use]
pub fn builtin_item_armour(identifier: &str) -> ItemArmour {
    static TABLE: OnceLock<BTreeMap<Box<str>, ItemArmour>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let source: Table =
            serde_json::from_slice(ITEM_ARMOUR_SOURCE_BYTES).expect("valid item armour source");
        assert_eq!(source.schema, 1, "unsupported item armour source");
        assert_eq!(source.protocol, crate::active_content_registry_protocol());
        let mut values = BTreeMap::new();
        for entry in source.items {
            assert!(entry.identifier.starts_with("minecraft:"));
            assert!(values.insert(entry.identifier, entry.armour).is_none());
        }
        values
    });
    table
        .get(identifier)
        .copied()
        .unwrap_or(ItemArmour::Unresolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_armour_facts_match_registered_items_and_missing_entries_stay_unresolved() {
        let source: Table = serde_json::from_slice(ITEM_ARMOUR_SOURCE_BYTES).unwrap();
        for entry in source.items {
            assert!(super::super::retail::identifiers().contains(entry.identifier.as_ref()));
            assert_eq!(builtin_item_armour(&entry.identifier), entry.armour);
        }
        assert_eq!(
            builtin_item_armour("minecraft:stick"),
            ItemArmour::Unresolved
        );
        assert_eq!(builtin_item_armour("iron_helmet"), ItemArmour::Unresolved);
    }
}
