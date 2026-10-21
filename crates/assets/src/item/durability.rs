use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};

pub const ITEM_DURABILITY_SOURCE_PATH: &str = "registry/item-durability-v2193.json";
pub const ITEM_DURABILITY_SOURCE_BYTES: &[u8] =
    include_bytes!("../../data/item-durability-v2193.json");

/// Effective item durability; missing evidence is distinct from a nondamageable item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "support", deny_unknown_fields)]
pub enum ItemDurability {
    Unresolved,
    NotDamageable,
    /// An admitted maximum, including zero for a component that remains damageable.
    Maximum {
        maximum: u32,
    },
}

impl ItemDurability {
    /// Interprets a parsed maximum: component zero remains damageable, while a legacy
    /// zero or missing maximum remains unresolved.
    pub const fn from_declared_maximum(maximum: Option<u32>, component_item: bool) -> Self {
        match maximum {
            Some(maximum) if component_item || maximum > 0 => Self::Maximum { maximum },
            _ => Self::Unresolved,
        }
    }

    /// Reports admitted damageability independently of a positive display or repair maximum.
    pub const fn damageable(self) -> Option<bool> {
        match self {
            Self::Unresolved => None,
            Self::NotDamageable => Some(false),
            Self::Maximum { .. } => Some(true),
        }
    }

    /// Returns a positive admitted maximum; unresolved and nondamageable items have no bar.
    pub const fn maximum(self) -> Option<u32> {
        match self {
            Self::Maximum { maximum } if maximum > 0 => Some(maximum),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Table {
    schema: u32,
    maximums: Box<[Entry]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    identifier: Box<str>,
    maximum: u32,
}

/// Resolves pinned base facts at compilation or registry admission, never during a frame.
pub fn builtin_item_durability(identifier: &str) -> ItemDurability {
    static TABLE: OnceLock<BTreeMap<Box<str>, ItemDurability>> = OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let source: Table = serde_json::from_slice(ITEM_DURABILITY_SOURCE_BYTES)
            .expect("valid item durability source");
        assert_eq!(source.schema, 1, "unsupported item durability source");
        let mut values = BTreeMap::new();
        for entry in source.maximums {
            assert!(entry.identifier.starts_with("minecraft:") && entry.maximum > 0);
            assert!(
                values
                    .insert(
                        entry.identifier,
                        ItemDurability::Maximum {
                            maximum: entry.maximum
                        }
                    )
                    .is_none()
            );
        }
        values
    });
    table
        .get(identifier)
        .copied()
        .unwrap_or(ItemDurability::Unresolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durability_positive_inventory_is_admitted_and_unknowns_are_explicit() {
        let inventory = include_str!("../../../protocol/data/retail_items_1_26_50.tsv");
        let identifiers = inventory
            .lines()
            .map(|line| line.split_once('\t').unwrap().1)
            .collect::<std::collections::BTreeSet<_>>();
        let source: Table = serde_json::from_slice(ITEM_DURABILITY_SOURCE_BYTES).unwrap();
        for entry in source.maximums {
            assert!(
                identifiers.contains(entry.identifier.as_ref()),
                "unadmitted durability {}",
                entry.identifier
            );
            assert_eq!(
                builtin_item_durability(&entry.identifier).maximum(),
                Some(entry.maximum)
            );
        }
        assert_eq!(
            builtin_item_durability("custom:iron_sword"),
            ItemDurability::Unresolved
        );
        assert_eq!(
            builtin_item_durability("minecraft:missing"),
            ItemDurability::Unresolved
        );
    }
    #[test]
    fn copper_durability_retains_the_pinned_values() {
        for tool in ["sword", "pickaxe", "axe", "shovel", "hoe"] {
            assert_eq!(
                builtin_item_durability(&format!("minecraft:copper_{tool}")).maximum(),
                Some(190)
            );
        }
        for (piece, maximum) in [
            ("helmet", 121),
            ("chestplate", 176),
            ("leggings", 165),
            ("boots", 143),
        ] {
            assert_eq!(
                builtin_item_durability(&format!("minecraft:copper_{piece}")).maximum(),
                Some(maximum)
            );
        }
    }
}
