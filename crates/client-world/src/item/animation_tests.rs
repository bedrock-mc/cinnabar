use super::*;
use assets::facts::ItemTraits;

#[test]
fn registry_admission_and_replacement_keep_retained_chestplates_and_leggings() {
    let mut chest = retail_entry("minecraft:iron_chestplate");
    let mut legs = retail_entry("minecraft:iron_leggings");
    chest.network_id = 12_001;
    legs.network_id = 12_002;
    let stack = |network_id| NetworkItemStack {
        network_id,
        count: 1,
        stack_network_id: -1,
        ..NetworkItemStack::empty()
    };
    let mut store = ItemStateStore::diagnostic();
    let lifetime = ActorLifetimeId {
        session_id: 1,
        dimension: 0,
        runtime_id: 2,
        spawn_revision: 1,
    };
    assert!(store.apply_armor(
        lifetime,
        1,
        &ArmorEquipmentEvent {
            actor_runtime_id: 2,
            helmet: NetworkItemStack::empty(),
            chestplate: stack(chest.network_id),
            leggings: stack(legs.network_id),
            boots: NetworkItemStack::empty(),
            body: NetworkItemStack::empty(),
        }
    ));
    for entries in [
        vec![chest.clone(), legs.clone()],
        vec![legs.clone(), chest.clone()],
    ] {
        assert!(store.apply_registry(ItemRegistryEvent {
            entries: entries.into()
        }));
        let armour = store.armor(2).unwrap();
        for (piece, entry) in [(&armour.chestplate, &chest), (&armour.leggings, &legs)] {
            assert_eq!(
                piece.item.identifier.as_deref(),
                Some(entry.identifier.as_ref())
            );
            assert!(!piece.item.identity.is_empty());
            assert!(store.animation_fact(entry.network_id).armour_slot.is_some());
        }
    }
}

/// Clones a pinned retail declaration without changing its admission evidence.
fn retail_entry(identifier: &str) -> ItemRegistryEntry {
    protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap()
        .clone()
}

#[test]
fn large_server_registries_keep_animation_facts_and_retail_fallbacks() {
    let templates = [
        retail_entry("minecraft:iron_sword"),
        retail_entry("minecraft:leather_chestplate"),
        retail_entry("minecraft:elytra"),
    ];
    let mut store = ItemStateStore::diagnostic();
    for count in [16_000, protocol::MAX_ITEM_REGISTRY_ENTRIES] {
        let entries: Arc<[_]> = (0..count)
            .map(|index| {
                let mut entry = templates[index % templates.len()].clone();
                entry.network_id = 10_000 + index as i32;
                entry
            })
            .collect();
        assert!(store.apply_registry(ItemRegistryEvent { entries }));
        for index in 0..count {
            let fact = store.animation_fact(10_000 + index as i32);
            match index % templates.len() {
                0 => assert!(fact.traits.contains(ItemTraits::SWORD)),
                1 => {
                    assert!(fact.is_leather());
                    assert_eq!(fact.armor_texture_index(), 1);
                }
                2 => assert!(fact.traits.contains(ItemTraits::GLIDER)),
                _ => unreachable!(),
            }
        }
        assert!(
            store
                .animation_fact(templates[0].network_id)
                .traits
                .contains(ItemTraits::SWORD)
        );
    }
}

#[test]
fn advertised_unresolved_and_component_items_shadow_retail_facts() {
    let sword = retail_entry("minecraft:iron_sword");
    let mut unknown_version = sword.clone();
    unknown_version.version = ItemRegistryVersion::Unknown(42);
    let mut missing_payload = sword.clone();
    missing_payload.components = None;
    missing_payload.canonical_empty_component_data = false;
    let mut component_item = sword.clone();
    component_item.component_based = true;
    component_item.version = ItemRegistryVersion::DataDriven;
    component_item.components = Some(Arc::new(protocol::ItemComponents::default()));
    let mut other_native = retail_entry("minecraft:stone");
    other_native.network_id = sword.network_id;
    let mut store = ItemStateStore::diagnostic();
    for replacement in [
        unknown_version,
        missing_payload,
        component_item,
        other_native,
    ] {
        assert!(store.apply_registry(ItemRegistryEvent {
            entries: Arc::from([replacement])
        }));
        assert!(
            !store
                .animation_fact(sword.network_id)
                .traits
                .contains(ItemTraits::SWORD)
        );
    }
    assert!(store.apply_registry(ItemRegistryEvent {
        entries: Arc::from([])
    }));
    assert!(
        store
            .animation_fact(sword.network_id)
            .traits
            .contains(ItemTraits::SWORD)
    );
}

#[test]
fn rejected_numeric_span_preserves_the_previous_animation_admission() {
    let sword = retail_entry("minecraft:iron_sword");
    let mut store = ItemStateStore::diagnostic();
    assert!(store.apply_registry(ItemRegistryEvent {
        entries: Arc::from([sword.clone()])
    }));
    let mut distant_alias = sword.clone();
    distant_alias.network_id = i32::MAX;
    assert!(!store.apply_registry(ItemRegistryEvent {
        entries: Arc::from([sword.clone(), distant_alias])
    }));
    assert_eq!(store.skipped_registries, 1);
    assert!(!store.apply_registry(ItemRegistryEvent {
        entries: Arc::from([sword.clone(), sword.clone()])
    }));
    assert_eq!(store.skipped_registries, 2);
    assert!(
        store
            .animation_fact(sword.network_id)
            .traits
            .contains(ItemTraits::SWORD)
    );
    assert_eq!(
        store.registry.len(),
        protocol::vanilla_item_registry().len()
    );
}
