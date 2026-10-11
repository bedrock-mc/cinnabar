use assets::{RuntimeEntityAssets, item::armour::ItemArmour, item::armour::ItemArmourSlot};
use protocol::{ItemRegistryEntry, ItemRegistryVersion};

/// Resolves protection at registry admission; malformed or unsupported facts stay unresolved.
pub(super) fn resolve(
    entry: &ItemRegistryEntry,
    assets: Option<&RuntimeEntityAssets>,
) -> ItemArmour {
    if matches!(entry.version, ItemRegistryVersion::Unknown(_)) {
        return ItemArmour::Unresolved;
    }
    if entry.component_based {
        let Some(components) = &entry.components else {
            return ItemArmour::Unresolved;
        };
        if !components.wearable_declared {
            return ItemArmour::NotArmour;
        }
        let slot = match components.wearable_slot.as_deref() {
            Some("slot.armor.head") => ItemArmourSlot::Head,
            Some("slot.armor.chest") => ItemArmourSlot::Chest,
            Some("slot.armor.legs") => ItemArmourSlot::Legs,
            Some("slot.armor.feet") => ItemArmourSlot::Feet,
            _ => return ItemArmour::Unresolved,
        };
        return components
            .wearable_protection
            .map_or(ItemArmour::Unresolved, |protection| ItemArmour::Wearable {
                slot,
                protection,
            });
    }
    let Some(assets) = assets else {
        return ItemArmour::Unresolved;
    };
    let key = assets::ItemVisualKey {
        identifier: entry.identifier.as_ref().into(),
        metadata: 0,
    };
    assets
        .item_visuals()
        .binary_search_by(|visual| visual.key.cmp(&key))
        .ok()
        .map_or(ItemArmour::Unresolved, |index| {
            assets.item_visuals()[index].armour
        })
}

impl super::ItemStateStore {
    /// Reads protection by numeric registry ID without inspecting stack NBT or item names.
    pub(crate) fn item_armour(&self, network_id: i32) -> ItemArmour {
        self.registry
            .get(&network_id)
            .map_or(ItemArmour::Unresolved, |record| record.armour)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Builds an admitted server entry with explicit wearable facts.
    fn wearable(identifier: &str, network_id: i32, protection: u32) -> ItemRegistryEntry {
        let mut entry = protocol::vanilla_item_registry()[0].clone();
        entry.identifier = Arc::from(identifier);
        entry.network_id = network_id;
        entry.component_based = true;
        entry.components = Some(Arc::new(protocol::ItemComponents {
            wearable_declared: true,
            wearable_slot: Some(Arc::from("slot.armor.chest")),
            wearable_protection: Some(protection),
            ..Default::default()
        }));
        entry
    }

    #[test]
    fn registry_replacement_resolves_custom_protection_for_every_numeric_alias() {
        let mut store = super::super::ItemStateStore::diagnostic();
        let entries = [
            wearable("custom:cloth", 10_001, 6),
            wearable("custom:cloth", 10_002, 6),
        ];
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: entries.into()
        }));
        for id in [10_001, 10_002] {
            assert_eq!(
                store.item_armour(id),
                ItemArmour::Wearable {
                    slot: ItemArmourSlot::Chest,
                    protection: 6
                }
            );
        }
        let mut replacement = wearable("custom:cloth", 10_001, 0);
        replacement.components = Some(Arc::new(protocol::ItemComponents::default()));
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: vec![replacement].into()
        }));
        assert_eq!(store.item_armour(10_001), ItemArmour::NotArmour);
        assert_eq!(store.item_armour(10_002), ItemArmour::Unresolved);
    }

    #[test]
    fn missing_values_and_unsupported_slots_do_not_become_zero_protection() {
        let mut entry = wearable("custom:cloth", 10_001, 0);
        assert_eq!(resolve(&entry, None).protection(), Some(0));
        Arc::make_mut(entry.components.as_mut().unwrap()).wearable_protection = None;
        assert_eq!(resolve(&entry, None), ItemArmour::Unresolved);
        Arc::make_mut(entry.components.as_mut().unwrap()).wearable_protection = Some(6);
        Arc::make_mut(entry.components.as_mut().unwrap()).wearable_slot =
            Some(Arc::from("slot.armor.body"));
        assert_eq!(resolve(&entry, None), ItemArmour::Unresolved);
        entry.version = ItemRegistryVersion::Unknown(42);
        assert_eq!(resolve(&entry, None), ItemArmour::Unresolved);
    }
}
