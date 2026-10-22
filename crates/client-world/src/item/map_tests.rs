use super::*;

/// Builds an original filled-map stack with signed identity and a checked data digest.
fn stack(network_id: i32, map_id: i64) -> NetworkItemStack {
    let mut extra = vec![255, 255, 1, 10, 0, 0, 4, 8, 0];
    extra.extend_from_slice(b"map_uuid");
    extra.extend_from_slice(&map_id.to_le_bytes());
    extra.push(0);
    NetworkItemStack {
        network_id,
        count: 1,
        nbt_digest: Sha256::digest(&extra).into(),
        extra_data: extra.into(),
        ..NetworkItemStack::empty()
    }
}

#[test]
fn negotiated_map_identity_survives_modern_empty_definition_rebinding() {
    let mut store = ItemStateStore::diagnostic();
    let mut entry = protocol::vanilla_item_registry()[0].clone();
    entry.network_id = 10_266;
    entry.identifier = assets::facts::FILLED_MAP_IDENTIFIER.into();
    entry.component_based = true;
    entry.version = protocol::ItemRegistryVersion::DataDriven;
    entry.components = None;
    entry.canonical_empty_component_data = true;
    let lifetime = ActorLifetimeId {
        session_id: 1,
        dimension: 0,
        runtime_id: 2,
        spawn_revision: 1,
    };
    let source = stack(entry.network_id, i64::MIN);
    assert!(store.apply_registry(ItemRegistryEvent {
        entries: vec![entry.clone()].into()
    }));
    for hand in HANDS {
        assert!(store.apply_equipment(
            lifetime,
            1,
            EquipmentEvent {
                actor_runtime_id: lifetime.runtime_id,
                stack: source.clone(),
                inventory_slot: 0,
                selected_slot: 0,
                window_id: if hand == ActorHandedness::Left {
                    protocol::OFFHAND_WINDOW_ID.try_into().unwrap()
                } else {
                    0
                },
                handedness: Some(hand),
            }
        ));
    }
    for components in [None, Some(Arc::new(protocol::ItemComponents::default()))] {
        entry.components = components;
        assert!(store.apply_registry(ItemRegistryEvent {
            entries: vec![entry.clone()].into()
        }));
        for hand in HANDS {
            let held = &store.get_in_hand(lifetime, hand).unwrap().item;
            assert_eq!(
                held.identifier.as_deref(),
                Some(assets::facts::FILLED_MAP_IDENTIFIER)
            );
            assert_eq!(held.identity.network_id, source.network_id);
            assert_eq!(held.identity.count, 1);
            assert_eq!(held.map_id, Some(i64::MIN));
        }
    }
}
