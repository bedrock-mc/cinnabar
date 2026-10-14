use super::*;
use assets::facts::{ContainerRoles, ItemTier, ItemTraits};

/// Selects the real pinned registry entry so fixtures retain valid identities.
fn entry(name: &str) -> ItemRegistryEntry {
    protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == name)
        .unwrap()
        .clone()
}

/// Builds a registry event from real entries and explicit component declarations.
fn event(entries: Vec<ItemRegistryEntry>) -> ItemRegistryEvent {
    ItemRegistryEvent {
        entries: entries.into(),
    }
}

/// Reads the checked builtin facts without requiring local artwork carriers.
fn pinned() -> &'static ItemFacts {
    ItemFacts::builtin().unwrap()
}

#[test]
fn registry_admission_copies_pinned_facts_after_id_remapping() {
    let registry = protocol::vanilla_item_registry();
    let entries: Vec<_> = registry
        .iter()
        .cloned()
        .map(|mut entry| {
            entry.network_id += 10_000;
            entry
        })
        .collect();
    let facts = SessionItemFacts::from_registry(pinned(), &event(entries)).unwrap();
    for entry in registry.iter() {
        assert_eq!(
            facts.get(entry.network_id + 10_000),
            pinned().by_identifier(&entry.identifier).unwrap()
        );
        assert_eq!(
            facts.retail_id(entry.network_id + 10_000),
            Some(entry.network_id)
        );
    }
    assert_eq!(*facts.get(i32::MAX), ItemFact::default());
}

#[test]
fn component_entries_derive_tags_and_equipment_without_inheriting_their_names() {
    let mut custom = entry("minecraft:stick");
    let armour = *pinned().by_identifier("minecraft:iron_chestplate").unwrap();
    custom.component_based = true;
    custom.version = ItemRegistryVersion::DataDriven;
    custom.item_tags = [
        Arc::from("minecraft:trim_materials"),
        Arc::from("minecraft:is_pickaxe"),
        Arc::from("minecraft:copper_tier"),
    ]
    .into();
    custom.components = Some(Arc::new(protocol::ItemComponents {
        wearable_declared: true,
        wearable_slot: Some(Arc::from("slot.armor.chest")),
        wearable_protection: armour.armour.protection(),
        max_stack_size_component_declared: true,
        max_stack_size: Some(1),
        allow_off_hand_declared: true,
        allow_off_hand: Some(false),
        use_animation: Some(Arc::from("bow")),
        use_duration_ticks: Some(32),
        ..Default::default()
    }));
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![custom.clone()])).unwrap();
    let fact = facts.get(custom.network_id);
    assert_eq!(facts.retail_id(custom.network_id), None);
    assert_eq!(fact.armour, armour.armour);
    assert_eq!(fact.armour_slot, armour.armour_slot);
    assert_eq!(fact.max_stack_size, Some(1));
    assert_eq!(fact.allow_off_hand, Some(false));
    assert_eq!(fact.use_animation, Some(ItemUseAnimationFact::Bow));
    assert_eq!(fact.use_duration_ticks, Some(32));
    assert!(fact.roles.contains(ContainerRoles::SMITHING_MATERIAL));
    assert!(fact.traits.contains(ItemTraits::PICKAXE));
    assert_eq!(fact.tier, Some(ItemTier::Copper));
    let mut replacement = entry("minecraft:elytra");
    replacement.component_based = true;
    replacement.version = ItemRegistryVersion::DataDriven;
    replacement.components = Some(Arc::new(protocol::ItemComponents::default()));
    let facts =
        SessionItemFacts::from_registry(pinned(), &event(vec![replacement.clone()])).unwrap();
    assert!(
        !facts
            .get(replacement.network_id)
            .traits
            .contains(ItemTraits::GLIDER)
    );
    assert_eq!(facts.get(replacement.network_id).armour_slot, None);
    assert_eq!(
        facts.get(replacement.network_id).max_stack_size,
        Some(protocol::ITEM_DEFAULT_MAX_STACK_SIZE)
    );
}

#[test]
fn malformed_and_unknown_components_leave_facts_unresolved() {
    let mut item = entry("minecraft:shield");
    item.component_based = true;
    item.components = Some(Arc::new(protocol::ItemComponents {
        wearable_declared: true,
        wearable_slot: Some(Arc::from("slot.armor.head")),
        allow_off_hand_declared: true,
        durability_declared: true,
        stack_capacity_ambiguous: true,
        ..Default::default()
    }));
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![item.clone()])).unwrap();
    let fact = facts.get(item.network_id);
    assert_eq!(fact.armour_slot, Some(ItemArmourSlot::Head));
    assert_eq!(fact.armour, ItemArmour::Unresolved);
    assert_eq!(fact.durability, ItemDurability::Unresolved);
    assert_eq!(fact.allow_off_hand, None);
    assert_eq!(fact.max_stack_size, None);
    item.version = ItemRegistryVersion::Unknown(99);
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![item.clone()])).unwrap();
    assert_eq!(*facts.get(item.network_id), ItemFact::default());
    assert_eq!(facts.retail_id(item.network_id), None);
    item.version = ItemRegistryVersion::DataDriven;
    item.components = None;
    item.canonical_empty_component_data = false;
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![item.clone()])).unwrap();
    assert_eq!(*facts.get(item.network_id), ItemFact::default());
}

#[test]
fn ledger_replacement_refreshes_facts_and_reconnect_retires_them() {
    let mut session = crate::InventorySession::new(1);
    let banner = entry("minecraft:banner");
    session
        .ledger_mut()
        .apply_registry(&event(vec![banner.clone()]));
    assert!(
        session
            .item_facts()
            .get(banner.network_id)
            .roles
            .contains(ContainerRoles::LOOM_BANNER)
    );
    let mut sword = entry("minecraft:diamond_sword");
    sword.network_id = banner.network_id;
    session
        .ledger_mut()
        .apply_registry(&event(vec![sword.clone()]));
    assert!(
        !session
            .item_facts()
            .get(sword.network_id)
            .roles
            .contains(ContainerRoles::LOOM_BANNER)
    );
    assert!(
        session
            .item_facts()
            .get(sword.network_id)
            .traits
            .contains(ItemTraits::SWORD)
    );
    session
        .ledger_mut()
        .apply_registry(&event(vec![sword.clone(), sword.clone()]));
    assert!(
        session
            .item_facts()
            .get(sword.network_id)
            .traits
            .contains(ItemTraits::SWORD)
    );
    let snapshot = session.ledger_snapshot();
    sword.component_based = true;
    sword.version = ItemRegistryVersion::DataDriven;
    sword.components = Some(Arc::new(protocol::ItemComponents::default()));
    sword.item_tags = [Arc::from("minecraft:banner")].into();
    session
        .ledger_mut()
        .apply_registry(&event(vec![sword.clone()]));
    assert!(
        session
            .item_facts()
            .get(sword.network_id)
            .roles
            .contains(ContainerRoles::LOOM_BANNER)
    );
    assert!(
        !session
            .item_facts()
            .get(sword.network_id)
            .traits
            .contains(ItemTraits::SWORD)
    );
    assert!(
        snapshot
            .item_facts()
            .get(sword.network_id)
            .traits
            .contains(ItemTraits::SWORD)
    );
    session.begin_session(2);
    assert_eq!(
        *session.item_facts().get(sword.network_id),
        ItemFact::default()
    );
}

#[test]
fn registry_id_spans_are_bounded_and_sparse_gaps_stay_unresolved() {
    let mut first = entry("minecraft:banner");
    let mut last = entry("minecraft:stick");
    first.network_id = i32::MIN;
    last.network_id = i32::MAX;
    assert!(
        SessionItemFacts::from_registry(pinned(), &event(vec![first.clone(), last.clone()]))
            .is_none()
    );
    first.network_id = -1;
    last.network_id = 1;
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![last, first])).unwrap();
    assert_eq!(*facts.get(0), ItemFact::default());
    assert!(facts.get(-1).roles.contains(ContainerRoles::LOOM_BANNER));
}
