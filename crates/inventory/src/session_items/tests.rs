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
fn filled_map_offhand_compatibility_respects_component_and_version_boundaries() {
    let identifier = assets::facts::FILLED_MAP_IDENTIFIER;
    assert!(assets::item::names::serialization_ids(identifier).is_some());
    let mut item = entry("minecraft:empty_map");
    item.identifier = identifier.into();
    item.network_id += 10_000;
    for (component_based, version, declared, permission, expected) in [
        (false, ItemRegistryVersion::None, false, None, Some(true)),
        (
            false,
            ItemRegistryVersion::Legacy,
            true,
            Some(false),
            Some(false),
        ),
        (false, ItemRegistryVersion::Legacy, true, None, None),
        (true, ItemRegistryVersion::DataDriven, false, None, None),
        (
            true,
            ItemRegistryVersion::DataDriven,
            true,
            Some(true),
            Some(true),
        ),
        (false, ItemRegistryVersion::Unknown(99), false, None, None),
    ] {
        item.component_based = component_based;
        item.version = version;
        item.components = Some(Arc::new(protocol::ItemComponents {
            allow_off_hand_declared: declared,
            allow_off_hand: permission,
            ..Default::default()
        }));
        let facts = SessionItemFacts::from_registry(pinned(), &event(vec![item.clone()])).unwrap();
        assert_eq!(facts.get(item.network_id).allow_off_hand, expected);
        assert_eq!(
            facts.get(item.network_id).offhand_provisional,
            expected == Some(true) && !declared
        );
        assert_eq!(facts.retail_id(item.network_id), None);
    }
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
fn cached_offhand_facts_follow_equipment_admission_precedence() {
    for component_based in [false, true] {
        for (wearable, permission, declared, expected) in [
            (Some("slot.weapon.offhand"), Some(false), true, Some(true)),
            (Some("slot.weapon.offhand"), None, true, Some(true)),
            (Some("slot.armor.head"), Some(false), true, Some(false)),
            (None, Some(true), true, Some(true)),
            (None, Some(false), true, Some(false)),
            (None, None, true, None),
        ] {
            let mut item = entry("minecraft:shield");
            item.component_based = component_based;
            item.components = Some(Arc::new(protocol::ItemComponents {
                wearable_declared: wearable.is_some(),
                wearable_slot: wearable.map(Arc::from),
                allow_off_hand: permission,
                allow_off_hand_declared: declared,
                ..Default::default()
            }));
            let facts =
                SessionItemFacts::from_registry(pinned(), &event(vec![item.clone()])).unwrap();
            let fact = facts.get(item.network_id);
            assert_eq!(fact.allow_off_hand, expected);
            assert!(!fact.offhand_provisional);
        }
    }
}

#[test]
fn hand_flag_overrides_preserve_pinned_rod_artwork() {
    let pinned_grip = pinned()
        .by_identifier("minecraft:carrot_on_a_stick")
        .unwrap()
        .grip;
    assert!(pinned_grip.mirrored_art());
    for upright in [true, false] {
        let mut rod = entry("minecraft:carrot_on_a_stick");
        rod.components = Some(Arc::new(protocol::ItemComponents {
            hand_equipped: Some(upright),
            hand_equipped_declared: true,
            ..Default::default()
        }));
        let facts = SessionItemFacts::from_registry(pinned(), &event(vec![rod.clone()])).unwrap();
        let grip = facts.get(rod.network_id).grip;
        assert_eq!(grip.upright(), Some(upright));
        assert_eq!(grip.mirrored_art(), pinned_grip.mirrored_art());
        rod.component_based = true;
        let facts = SessionItemFacts::from_registry(pinned(), &event(vec![rod.clone()])).unwrap();
        assert!(!facts.get(rod.network_id).grip.mirrored_art());
    }
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

#[test]
fn declared_body_and_offhand_equipment_rows_are_cached() {
    for (slot, row, offhand) in [
        ("slot.armor.body", Some(4), None),
        ("slot.weapon.offhand", None, Some(true)),
    ] {
        let mut item = entry("minecraft:stick");
        item.component_based = true;
        item.version = ItemRegistryVersion::DataDriven;
        item.components = Some(Arc::new(protocol::ItemComponents {
            wearable_declared: true,
            wearable_slot: Some(Arc::from(slot)),
            ..Default::default()
        }));
        let facts = SessionItemFacts::from_registry(pinned(), &event(vec![item.clone()])).unwrap();
        assert_eq!(facts.armour_row(item.network_id), row);
        assert_eq!(facts.get(item.network_id).allow_off_hand, offhand);
    }
}

#[test]
fn enchanted_book_output_tracks_the_negotiated_registry() {
    let mut book = entry("minecraft:enchanted_book");
    book.network_id += 10_000;
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![book.clone()])).unwrap();
    assert_eq!(facts.enchanted_book_id(), Some(book.network_id));
    book.component_based = true;
    book.version = ItemRegistryVersion::DataDriven;
    book.components = Some(Arc::new(protocol::ItemComponents::default()));
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![book])).unwrap();
    assert_eq!(facts.enchanted_book_id(), None);
}

#[test]
fn anvil_materials_use_session_tags_and_independent_retail_identity() {
    let mut target = entry("minecraft:stick");
    target.component_based = true;
    target.version = ItemRegistryVersion::DataDriven;
    target.item_tags = [
        Arc::from("minecraft:wooden_tier"),
        Arc::from("minecraft:is_pickaxe"),
    ]
    .into();
    target.components = Some(Arc::new(protocol::ItemComponents {
        durability_declared: true,
        max_durability: Some(100),
        ..Default::default()
    }));
    let mut planks = entry("minecraft:oak_planks");
    planks.component_based = true;
    planks.version = ItemRegistryVersion::DataDriven;
    planks.item_tags = [Arc::from("minecraft:planks")].into();
    planks.components = Some(Arc::new(protocol::ItemComponents::default()));
    let mut sword = entry("minecraft:diamond_sword");
    let mut diamond = entry("minecraft:diamond");
    sword.network_id += 10_000;
    diamond.network_id += 10_000;
    let facts = SessionItemFacts::from_registry(
        pinned(),
        &event(vec![
            target.clone(),
            planks.clone(),
            sword.clone(),
            diamond.clone(),
        ]),
    )
    .unwrap();
    let left = crate::AnvilItem {
        kind: facts.kind(target.network_id),
        retail_id: facts.retail_id(target.network_id),
        fact: *facts.get(target.network_id),
        max_damage: facts.get(target.network_id).durability.maximum(),
        count: 1,
        damage: 40,
        ..Default::default()
    };
    let right = crate::AnvilItem {
        kind: facts.kind(planks.network_id),
        retail_id: facts.retail_id(planks.network_id),
        fact: *facts.get(planks.network_id),
        count: 2,
        ..Default::default()
    };
    let outcome = crate::evaluate_anvil(&left, Some(&right), "", "", false).unwrap();
    assert_eq!(outcome.damage, Some(0));
    assert_eq!(outcome.material_used, 2);
    assert_eq!(outcome.cost, 2);
    assert!(facts.get(target.network_id).repairs_with(
        facts.get(planks.network_id),
        facts.retail_id(planks.network_id)
    ));
    assert!(facts.get(sword.network_id).repairs_with(
        facts.get(diamond.network_id),
        facts.retail_id(diamond.network_id)
    ));
    diamond.component_based = true;
    diamond.version = ItemRegistryVersion::DataDriven;
    diamond.components = Some(Arc::new(protocol::ItemComponents::default()));
    let facts =
        SessionItemFacts::from_registry(pinned(), &event(vec![sword.clone(), diamond.clone()]))
            .unwrap();
    assert!(!facts.get(sword.network_id).repairs_with(
        facts.get(diamond.network_id),
        facts.retail_id(diamond.network_id)
    ));
}

#[test]
fn enchantment_slots_use_declared_classes_and_wearables() {
    let mut item = entry("minecraft:stick");
    item.component_based = true;
    item.version = ItemRegistryVersion::DataDriven;
    item.item_tags = [Arc::from("minecraft:is_sword")].into();
    item.components = Some(Arc::new(protocol::ItemComponents::default()));
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![item.clone()])).unwrap();
    assert_eq!(
        crate::enchantments::item_slot(facts.get(item.network_id)),
        crate::enchantments::slot::SWORD
    );
    item.item_tags = Arc::from([]);
    item.components = Some(Arc::new(protocol::ItemComponents {
        wearable_declared: true,
        wearable_slot: Some(Arc::from("slot.armor.feet")),
        ..Default::default()
    }));
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![item.clone()])).unwrap();
    assert_eq!(
        crate::enchantments::item_slot(facts.get(item.network_id)),
        crate::enchantments::slot::FEET
    );
    item.version = ItemRegistryVersion::Unknown(99);
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![item.clone()])).unwrap();
    assert_eq!(
        crate::enchantments::item_slot(facts.get(item.network_id)),
        0
    );
    assert_eq!(facts.armour_row(item.network_id), None);
}

#[test]
fn native_items_add_declared_roles_without_losing_exact_repair_overrides() {
    let mut mace = entry("minecraft:mace");
    mace.item_tags = [Arc::from("minecraft:horse_armor")].into();
    let facts = SessionItemFacts::from_registry(pinned(), &event(vec![mace.clone()])).unwrap();
    let fact = facts.get(mace.network_id);
    assert!(fact.roles.contains(ContainerRoles::HORSE_ARMOUR));
    assert_eq!(
        fact.repair,
        pinned().by_identifier(&mace.identifier).unwrap().repair
    );
    assert_eq!(
        crate::enchantments::item_slot(fact),
        crate::enchantments::slot::MACE
    );
}

#[test]
fn anvil_copy_combination_preserves_native_and_component_registry_aliases() {
    for component_based in [false, true] {
        let mut first = entry("minecraft:diamond_sword");
        first.component_based = component_based;
        if component_based {
            first.version = ItemRegistryVersion::DataDriven;
            first.components = Some(Arc::new(protocol::ItemComponents {
                durability_declared: true,
                max_durability: Some(200),
                ..Default::default()
            }));
        }
        let mut alias = first.clone();
        alias.network_id += 10_000;
        let facts =
            SessionItemFacts::from_registry(pinned(), &event(vec![first.clone(), alias.clone()]))
                .unwrap();
        assert_eq!(facts.kind(first.network_id), facts.kind(alias.network_id));
        let left = crate::AnvilItem {
            kind: facts.kind(first.network_id),
            fact: *facts.get(first.network_id),
            max_damage: facts.get(first.network_id).durability.maximum(),
            damage: 100,
            count: 1,
            ..Default::default()
        };
        let right = crate::AnvilItem {
            kind: facts.kind(alias.network_id),
            ..left.clone()
        };
        let outcome = crate::evaluate_anvil(&left, Some(&right), "", "", false).unwrap();
        assert!(outcome.damage.unwrap() < left.damage);
        assert_eq!(outcome.cost, 2);
    }
}

/// User data with a root integer `Damage`, as the stack's extra data carries it.
fn damaged_extra(damage: i32) -> Arc<[u8]> {
    let mut extra = (-1_i16).to_le_bytes().to_vec();
    extra.push(1);
    extra.extend([10, 0, 0, 3]);
    extra.extend(6_u16.to_le_bytes());
    extra.extend(b"Damage");
    extra.extend(damage.to_le_bytes());
    extra.push(0);
    extra.into()
}

#[test]
fn damaged_humanoid_armour_keys_its_stored_damage() {
    let helmet = entry("minecraft:iron_helmet");
    let sword = entry("minecraft:iron_sword");
    let mut component_helmet = entry("minecraft:diamond_helmet");
    component_helmet.component_based = true;
    component_helmet.version = ItemRegistryVersion::DataDriven;
    component_helmet.components = Some(Arc::new(protocol::ItemComponents::default()));
    let facts = SessionItemFacts::from_registry(
        pinned(),
        &event(vec![
            helmet.clone(),
            sword.clone(),
            component_helmet.clone(),
        ]),
    )
    .unwrap();
    let stack = |network_id| protocol::NetworkItemStack {
        network_id,
        metadata: 0,
        count: 1,
        extra_data: damaged_extra(7),
        ..Default::default()
    };
    assert!(facts.humanoid_armour(helmet.network_id));
    assert_eq!(
        facts.id_aux(&stack(helmet.network_id)),
        (helmet.network_id << 16) | 7
    );
    // Other damageable items, and component armour, keep the wire metadata.
    assert_eq!(
        facts.id_aux(&stack(sword.network_id)),
        sword.network_id << 16
    );
    assert!(!facts.humanoid_armour(component_helmet.network_id));
    assert_eq!(
        facts.id_aux(&stack(component_helmet.network_id)),
        component_helmet.network_id << 16
    );
}
