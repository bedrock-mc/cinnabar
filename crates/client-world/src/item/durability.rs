use assets::{RuntimeEntityAssets, item::durability::ItemDurability};
use protocol::{ItemRegistryEntry, ItemRegistryVersion};

/// Resolves component durability or the built-in or asset fallback, preserving unsupported states.
pub fn resolve_item_durability(
    entry: &ItemRegistryEntry,
    assets: Option<&RuntimeEntityAssets>,
) -> ItemDurability {
    if matches!(entry.version, ItemRegistryVersion::Unknown(_)) {
        return ItemDurability::Unresolved;
    }
    if let Some(components) = &entry.components {
        let durability = ItemDurability::from_declared_maximum(
            components.max_durability,
            entry.component_based || matches!(entry.version, ItemRegistryVersion::DataDriven),
        );
        if durability.damageable() == Some(true) || components.durability_declared {
            return durability;
        }
        if entry.component_based {
            return ItemDurability::NotDamageable;
        }
    } else if entry.component_based {
        return ItemDurability::Unresolved;
    }
    let Some(assets) = assets else {
        return assets::item::durability::builtin_item_durability(&entry.identifier);
    };
    let key = assets::ItemVisualKey {
        identifier: entry.identifier.as_ref().into(),
        metadata: 0,
    };
    assets
        .item_visuals()
        .binary_search_by(|visual| visual.key.cmp(&key))
        .ok()
        .map_or(ItemDurability::Unresolved, |index| {
            assets.item_visuals()[index].durability
        })
}

impl super::ItemStateStore {
    /// Reads effective durability by numeric ID without parsing components or matching names.
    pub(crate) fn item_durability(&self, network_id: i32) -> ItemDurability {
        self.registry
            .get(&network_id)
            .map_or(ItemDurability::Unresolved, |record| record.durability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn durability_replacement_retains_aliases_and_missing_values_are_unresolved() {
        let mut entry = protocol::vanilla_item_registry()[0].clone();
        entry.identifier = "custom:tool".into();
        entry.network_id = 10_001;
        entry.component_based = true;
        entry.components = Some(Arc::new(protocol::ItemComponents {
            durability_declared: true,
            max_durability: Some(50),
            ..Default::default()
        }));
        let mut alias = entry.clone();
        alias.network_id = 10_002;
        let mut store = super::super::ItemStateStore::diagnostic();
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: vec![entry.clone(), alias].into()
        }));
        for id in [10_001, 10_002] {
            assert_eq!(store.item_durability(id).maximum(), Some(50));
        }
        Arc::make_mut(entry.components.as_mut().unwrap()).max_durability = None;
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: vec![entry].into()
        }));
        assert_eq!(store.item_durability(10_001), ItemDurability::Unresolved);
        assert_eq!(store.item_durability(10_002), ItemDurability::Unresolved);
    }
    /// A valid zero component maximum remains damageable without a positive display maximum.
    #[test]
    fn zero_component_durability_survives_world_registry_admission() {
        let mut entry = protocol::vanilla_item_registry()[0].clone();
        entry.identifier = "fixture:zero_durability".into();
        entry.network_id = 10_001;
        entry.component_based = true;
        entry.version = ItemRegistryVersion::DataDriven;
        entry.components = Some(Arc::new(protocol::ItemComponents {
            durability_declared: true,
            max_durability: Some(0),
            ..Default::default()
        }));
        let mut store = super::super::ItemStateStore::diagnostic();
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: vec![entry].into()
        }));
        let durability = store.item_durability(10_001);
        assert_eq!(durability.damageable(), Some(true));
        assert_eq!(durability.maximum(), None);
    }

    #[test]
    fn durability_support_distinguishes_known_absence_from_missing_payload_and_versions() {
        let mut entry = protocol::vanilla_item_registry()[0].clone();
        entry.component_based = true;
        assert_eq!(
            resolve_item_durability(&entry, None),
            ItemDurability::Unresolved
        );
        entry.components = Some(Arc::new(protocol::ItemComponents::default()));
        assert_eq!(
            resolve_item_durability(&entry, None),
            ItemDurability::NotDamageable
        );
        entry.version = ItemRegistryVersion::Unknown(99);
        Arc::make_mut(entry.components.as_mut().unwrap()).max_durability = Some(50);
        assert_eq!(
            resolve_item_durability(&entry, None),
            ItemDurability::Unresolved
        );
    }
}
