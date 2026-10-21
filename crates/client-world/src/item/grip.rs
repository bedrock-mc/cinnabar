use assets::{RuntimeEntityAssets, item::grip::ItemGrip};
use protocol::{ItemRegistryEntry, ItemRegistryVersion};

#[cfg(test)]
#[path = "grip_empty_native_tests.rs"]
mod empty_native_tests;

/// Resolves grip overrides; canonical empty component data retains checked native facts.
pub(super) fn resolve(entry: &ItemRegistryEntry, assets: Option<&RuntimeEntityAssets>) -> ItemGrip {
    if matches!(entry.version, ItemRegistryVersion::Unknown(_)) {
        return ItemGrip::Unresolved;
    }
    let compiled = assets
        .and_then(|assets| {
            let key = assets::ItemVisualKey {
                identifier: entry.identifier.as_ref().into(),
                metadata: 0,
            };
            assets
                .item_visuals()
                .binary_search_by(|visual| visual.key.cmp(&key))
                .ok()
                .map(|index| assets.item_visuals()[index].grip)
        })
        .unwrap_or_default();
    let components = entry.components.as_deref();
    compiled.with_hand_equipped(
        components.and_then(|components| components.hand_equipped),
        components.is_some_and(|components| components.hand_equipped_declared),
        entry.component_based && !entry.canonical_empty_component_data,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Builds one custom registry declaration with an explicit or absent hand flag.
    fn entry(id: i32, upright: Option<bool>) -> ItemRegistryEntry {
        let mut entry = protocol::vanilla_item_registry()[0].clone();
        entry.network_id = id;
        entry.identifier = Arc::from("custom:diamond_sword");
        entry.component_based = true;
        entry.components = Some(Arc::new(protocol::ItemComponents {
            hand_equipped: upright,
            ..Default::default()
        }));
        entry
    }

    #[test]
    fn numeric_aliases_refresh_explicit_false_and_missing_grips() {
        let mut store = super::super::ItemStateStore::diagnostic();
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: vec![entry(10_001, Some(true)), entry(10_002, Some(true))].into()
        }));
        assert_eq!(store.registry[&10_001].grip.upright(), Some(true));
        assert_eq!(store.registry[&10_002].grip.upright(), Some(true));
        let lifetime = actor_animation::ActorLifetimeId {
            session_id: 1,
            dimension: 0,
            runtime_id: 1,
            spawn_revision: 1,
        };
        let mut stack = protocol::NetworkItemStack::empty();
        stack.network_id = 10_001;
        stack.count = 1;
        assert!(store.apply_equipment(
            lifetime,
            1,
            protocol::EquipmentEvent {
                actor_runtime_id: 1,
                stack,
                inventory_slot: 0,
                selected_slot: 0,
                window_id: 0,
                handedness: Some(protocol::ActorHandedness::Right),
            }
        ));
        let key = (lifetime, protocol::ActorHandedness::Right);
        assert_eq!(store.equipment[&key].item.grip.upright(), Some(true));
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: vec![entry(10_001, Some(false))].into()
        }));
        assert_eq!(store.registry[&10_001].grip.upright(), Some(false));
        assert_eq!(store.equipment[&key].item.grip.upright(), Some(false));
        assert!(!store.registry.contains_key(&10_002));
        assert!(store.apply_registry(protocol::ItemRegistryEvent {
            entries: vec![entry(10_001, None)].into()
        }));
        assert_eq!(store.registry[&10_001].grip, ItemGrip::Unresolved);
        assert_eq!(store.equipment[&key].item.grip, ItemGrip::Unresolved);
        let mut unsupported = entry(10_001, Some(true));
        unsupported.version = ItemRegistryVersion::Unknown(42);
        assert_eq!(resolve(&unsupported, None), ItemGrip::Unresolved);
    }
}
