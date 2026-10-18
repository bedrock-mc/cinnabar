//! Quick-move requests for items from the pinned registry and role data.

use assets::item::armour::{ITEM_ARMOUR_SOURCE_BYTES, ItemArmour, ItemArmourSlot};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent,
    ItemRegistryEntry, ItemRegistryEvent, NetworkItemStack, StackRequestAction,
    StackRequestContainer,
};
use serde::Deserialize;

use super::*;

#[path = "quick_move_role_tests/station_counts.rs"]
mod station_counts;

const SOURCE_SLOT: u8 = 20;

/// Starts a server-backed player inventory with one registered item and known UI cells.
fn ledger(entry: &ItemRegistryEntry, metadata: u32) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    let mut entries = protocol::vanilla_item_registry().to_vec();
    entries.retain(|existing| existing.network_id != entry.network_id);
    entries.push(entry.clone());
    ledger.apply_registry(&ItemRegistryEvent {
        entries: entries.into(),
    });
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[usize::from(SOURCE_SLOT)] = NetworkItemStack {
        network_id: entry.network_id,
        stack_network_id: 100,
        count: 1,
        metadata,
        ..NetworkItemStack::default()
    };
    ledger.apply(&content(ContainerIdentity::window(0), slots));
    ledger.apply(&content(
        ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
        vec![NetworkItemStack::default(); protocol::UI_SLOT_COUNT],
    ));
    ledger
}

/// Builds an authoritative content update for the requested container.
fn content(container: ContainerIdentity, slots: Vec<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: slots.into(),
        storage_item: NetworkItemStack::default(),
    })
}

/// Opens a server window without changing the registered item or player slots.
fn open(ledger: &mut PlayerInventoryLedger, window_type: i8) {
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(3),
        window_type,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
}

/// Opens a personal inventory with empty armour cells for a registered item.
fn equipment_ledger(entry: &ItemRegistryEntry) -> PlayerInventoryLedger {
    let mut ledger = ledger(entry, 0);
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    open(&mut ledger, PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&content(
        ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
        vec![NetworkItemStack::default(); usize::from(protocol::ARMOR_SLOTS)],
    ));
    ledger
}

/// Checks the request's destination and the predicted item's retained identity.
fn assert_quick_move(
    ledger: &mut PlayerInventoryLedger,
    entry: &ItemRegistryEntry,
    target: InventoryTarget,
) {
    let held = ledger.displayed_stack(SOURCE_SLOT).unwrap().clone();
    let expected = match target {
        InventoryTarget::Armor(row) => (StackRequestContainer::Armor, row),
        InventoryTarget::Craft(slot) => (protocol::ui_slot_request_container(slot).unwrap(), slot),
        InventoryTarget::Storage(slot) => {
            protocol::open_cell_request(ledger.window_kind().unwrap(), slot, None, None).unwrap()
        }
        InventoryTarget::Player(slot) => (StackRequestContainer::PlayerInventory, slot),
        InventoryTarget::Offhand => (StackRequestContainer::Offhand, 1),
    };
    ledger
        .begin_quick_move(InventoryTarget::Player(SOURCE_SLOT))
        .unwrap();
    let request = ledger.newest_request().unwrap();
    assert_eq!(request.actions.len(), 1, "{}", entry.identifier);
    let StackRequestAction::Place {
        amount,
        source,
        destination,
    } = &request.actions[0]
    else {
        panic!("expected a quick-move placement for {}", entry.identifier);
    };
    assert_eq!(*amount, 1);
    assert_eq!(source.container, StackRequestContainer::PlayerInventory);
    assert_eq!(source.slot, SOURCE_SLOT);
    assert_eq!(source.stack_network_id, held.stack_network_id);
    assert_eq!(
        (destination.container, destination.slot),
        expected,
        "{}",
        entry.identifier
    );
    assert!(ledger.displayed_stack(SOURCE_SLOT).is_none());
    let moved = ledger.target_stack(target).unwrap();
    assert_eq!(moved.network_id, held.network_id);
    assert_eq!(moved.metadata, held.metadata);
    assert_eq!(moved.count, 1);
}

/// Resolves a fixture identity from the actual pinned item registry.
fn registered(identifier: &str) -> ItemRegistryEntry {
    protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == identifier)
        .unwrap_or_else(|| panic!("missing pinned item {identifier}"))
        .clone()
}

#[test]
fn registered_filled_maps_admit_manual_and_quick_move_offhand_placement() {
    let identifier = assets::facts::FILLED_MAP_IDENTIFIER;
    assert!(assets::item::names::serialization_ids(identifier).is_some());
    assert!(
        assets::facts::ItemFacts::builtin()
            .unwrap()
            .by_identifier(identifier)
            .is_none()
    );
    let mut entry = registered("minecraft:empty_map");
    entry.identifier = identifier.into();
    entry.network_id += 10_000;
    for quick_move in [false, true] {
        let mut ledger = equipment_ledger(&entry);
        ledger.apply(&content(
            ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
            vec![NetworkItemStack::empty()],
        ));
        assert_eq!(
            ledger.item_facts().get(entry.network_id).allow_off_hand,
            Some(true)
        );
        assert!(
            ledger
                .item_facts()
                .get(entry.network_id)
                .offhand_provisional
        );
        if quick_move {
            assert_quick_move(&mut ledger, &entry, InventoryTarget::Offhand);
        } else {
            ledger.begin_click(SOURCE_SLOT).unwrap();
            ledger
                .begin_target_gesture(InventoryTarget::Offhand, CellGesture::Click)
                .unwrap();
            assert!(ledger.displayed_stack(SOURCE_SLOT).is_none());
            assert!(ledger.cursor_stack().is_none());
            let moved = ledger.target_stack(InventoryTarget::Offhand).unwrap();
            assert_eq!(moved.network_id, entry.network_id);
            assert_eq!(moved.count, 1);
        }
    }
}

/// Admits a server-defined item with a known role tag and an optional wearable slot.
fn tagged_entry(identifier: &str, tag: &str, wearable: Option<&str>) -> ItemRegistryEntry {
    assert!(protocol::vanilla_tag_contains(tag, identifier).is_some());
    let mut bytes = vec![10, 0, 10];
    nbt_string(&mut bytes, "components");
    bytes.push(9);
    nbt_string(&mut bytes, "item_tags");
    bytes.extend_from_slice(&[8, 2]);
    nbt_string(&mut bytes, tag);
    if let Some(slot) = wearable {
        bytes.push(10);
        nbt_string(&mut bytes, "minecraft:wearable");
        bytes.push(8);
        nbt_string(&mut bytes, "slot");
        nbt_string(&mut bytes, slot);
        bytes.push(0);
    }
    bytes.extend_from_slice(&[0, 0]);
    let event =
        protocol::test_support::items::component_item_registry_payload(identifier, 10_001, bytes);
    let entry = event.entries[0].clone();
    assert_eq!(
        entry.item_tags.as_ref(),
        &[std::sync::Arc::<str>::from(tag)]
    );
    entry
}

/// Resends the current registry with one existing item's declarations replaced.
fn resend(ledger: &mut PlayerInventoryLedger, entry: &ItemRegistryEntry) {
    assert!(ledger.negotiated_item_entry(entry.network_id).is_some());
    let entries: Vec<_> = ledger
        .negotiated_item_registry()
        .unwrap()
        .values()
        .map(|current| {
            if current.network_id == entry.network_id {
                entry.clone()
            } else {
                current.clone()
            }
        })
        .collect();
    ledger.apply_registry(&ItemRegistryEvent {
        entries: entries.into(),
    });
}

/// Writes a network-NBT string whose length fits the fixture's single-byte prefix.
fn nbt_string(bytes: &mut Vec<u8>, value: &str) {
    assert!(value.len() < 128);
    bytes.push(u8::try_from(value.len()).unwrap());
    bytes.extend_from_slice(value.as_bytes());
}

#[derive(Deserialize)]
struct ArmourTable {
    items: Vec<ArmourEntry>,
}

#[derive(Deserialize)]
struct ArmourEntry {
    identifier: String,
    armour: ItemArmour,
}

#[test]
fn every_pinned_armour_item_quick_moves_to_its_declared_row() {
    let table: ArmourTable = serde_json::from_slice(ITEM_ARMOUR_SOURCE_BYTES).unwrap();
    assert!(!table.items.is_empty());
    for item in table.items {
        let entry = registered(&item.identifier);
        let ItemArmour::Wearable { slot, .. } = item.armour else {
            continue;
        };
        let row = match slot {
            ItemArmourSlot::Head => 0,
            ItemArmourSlot::Chest => 1,
            ItemArmourSlot::Legs => 2,
            ItemArmourSlot::Feet => 3,
        };
        let mut ledger = equipment_ledger(&entry);
        assert_quick_move(&mut ledger, &entry, InventoryTarget::Armor(row));
    }
}

#[test]
fn registered_heads_and_carved_pumpkin_quick_move_to_the_head_slot() {
    let entries = protocol::vanilla_item_registry();
    let heads: Vec<_> = entries
        .iter()
        .filter(|entry| assets::vanilla_skull_type(&entry.identifier).is_some())
        .collect();
    assert!(!heads.is_empty());
    let pumpkin = registered("minecraft:carved_pumpkin");
    for entry in heads.into_iter().chain(std::iter::once(&pumpkin)) {
        let mut ledger = equipment_ledger(entry);
        assert_quick_move(&mut ledger, entry, InventoryTarget::Armor(0));
    }
}

#[test]
fn registered_banner_quick_moves_to_the_loom_without_losing_its_colour() {
    let entry = registered("minecraft:banner");
    let mut ledger = ledger(&entry, 11);
    open(&mut ledger, protocol::WINDOW_TYPE_LOOM);
    assert_quick_move(&mut ledger, &entry, InventoryTarget::Craft(9));
}

#[test]
fn smithing_tags_use_negotiated_members_and_pinned_fallback() {
    for (tag, slot) in [
        ("minecraft:trim_templates", 53),
        ("minecraft:transform_templates", 53),
        ("minecraft:trim_materials", 52),
        ("minecraft:transform_materials", 52),
        ("minecraft:trimmable_armors", 51),
        ("minecraft:transformable_items", 51),
    ] {
        let custom = tagged_entry("custom:ruby_ingot", tag, None);
        assert!(crate::screen_ingredient_accepts(
            &protocol::ScreenIngredient {
                name: tag.into(),
                tag: true,
                aux: protocol::RECIPE_ANY_AUX,
            },
            &custom.identifier,
            0,
            &custom.item_tags,
        ));
        let mut custom_ledger = ledger(&custom, 0);
        open(&mut custom_ledger, protocol::WINDOW_TYPE_SMITHING_TABLE);
        assert_quick_move(&mut custom_ledger, &custom, InventoryTarget::Craft(slot));

        let entries = protocol::vanilla_item_registry();
        let members: Vec<_> = entries
            .iter()
            .filter(|entry| protocol::vanilla_tag_contains(tag, &entry.identifier) == Some(true))
            .collect();
        assert!(!members.is_empty(), "missing pinned tag members for {tag}");
        for entry in members {
            let mut ledger = ledger(entry, 0);
            open(&mut ledger, protocol::WINDOW_TYPE_SMITHING_TABLE);
            assert_quick_move(&mut ledger, entry, InventoryTarget::Craft(slot));
        }
    }
}

#[test]
fn component_items_do_not_inherit_smithing_roles_from_a_stock_name() {
    let entry = tagged_entry("minecraft:netherite_ingot", "minecraft:is_food", None);
    let mut ledger = ledger(&entry, 0);
    open(&mut ledger, protocol::WINDOW_TYPE_SMITHING_TABLE);
    assert_quick_move(&mut ledger, &entry, InventoryTarget::Player(0));
}

#[test]
fn a_banner_tag_does_not_replace_the_loom_item_identity() {
    let entry = tagged_entry("custom:standard", "minecraft:banner", None);
    let mut ledger = ledger(&entry, 11);
    open(&mut ledger, protocol::WINDOW_TYPE_LOOM);
    assert_quick_move(&mut ledger, &entry, InventoryTarget::Player(0));
}

#[test]
fn custom_head_items_follow_the_wearable_slot_rather_than_a_general_armour_tag() {
    for wearable in [Some("slot.armor.head"), None] {
        let entry = tagged_entry("custom:mask", "minecraft:is_armor", wearable);
        let mut ledger = equipment_ledger(&entry);
        let target = if wearable.is_some() {
            InventoryTarget::Armor(0)
        } else {
            InventoryTarget::Player(0)
        };
        assert_quick_move(&mut ledger, &entry, target);
    }
}

#[test]
fn untagged_smithing_items_fall_back_to_the_player_inventory() {
    for identifier in ["minecraft:brick", "minecraft:stick", "minecraft:elytra"] {
        let entry = registered(identifier);
        let mut ledger = ledger(&entry, 0);
        open(&mut ledger, protocol::WINDOW_TYPE_SMITHING_TABLE);
        assert_quick_move(&mut ledger, &entry, InventoryTarget::Player(0));
    }
}

#[test]
fn horse_armour_tag_members_quick_move_to_the_mount_equipment_slot() {
    let entries = protocol::vanilla_item_registry();
    let members: Vec<_> = entries
        .iter()
        .filter(|entry| {
            protocol::vanilla_tag_contains("minecraft:horse_armor", &entry.identifier) == Some(true)
        })
        .collect();
    assert!(!members.is_empty());
    for entry in members {
        let mut ledger = ledger(entry, 0);
        open(&mut ledger, protocol::WINDOW_TYPE_HORSE);
        ledger.apply(&content(
            ContainerIdentity::window(3),
            vec![NetworkItemStack::default(); 2],
        ));
        assert_quick_move(&mut ledger, entry, InventoryTarget::Storage(1));
    }
}

#[test]
fn negotiated_horse_armour_tag_routes_a_custom_item_to_mount_equipment() {
    let entry = tagged_entry("custom:mount_guard", "minecraft:horse_armor", None);
    let mut ledger = ledger(&entry, 0);
    open(&mut ledger, protocol::WINDOW_TYPE_HORSE);
    ledger.apply(&content(
        ContainerIdentity::window(3),
        vec![NetworkItemStack::default(); 2],
    ));
    assert_quick_move(&mut ledger, &entry, InventoryTarget::Storage(1));
}

#[test]
fn registry_role_changes_retire_unsent_quick_moves_and_restore_the_source() {
    for (window, before, after, old_target, new_target) in [
        (
            protocol::WINDOW_TYPE_SMITHING_TABLE,
            "minecraft:trim_materials",
            "minecraft:trim_templates",
            InventoryTarget::Craft(52),
            InventoryTarget::Craft(53),
        ),
        (
            protocol::WINDOW_TYPE_SMITHING_TABLE,
            "minecraft:trim_materials",
            "minecraft:is_food",
            InventoryTarget::Craft(52),
            InventoryTarget::Player(0),
        ),
        (
            protocol::WINDOW_TYPE_SMITHING_TABLE,
            "minecraft:is_food",
            "minecraft:trim_materials",
            InventoryTarget::Player(0),
            InventoryTarget::Craft(52),
        ),
        (
            protocol::WINDOW_TYPE_HORSE,
            "minecraft:horse_armor",
            "minecraft:is_food",
            InventoryTarget::Storage(1),
            InventoryTarget::Player(0),
        ),
        (
            protocol::WINDOW_TYPE_HORSE,
            "minecraft:is_food",
            "minecraft:horse_armor",
            InventoryTarget::Player(0),
            InventoryTarget::Storage(1),
        ),
    ] {
        let entry = tagged_entry("custom:ruby_ingot", before, None);
        let mut ledger = ledger(&entry, 0);
        open(&mut ledger, window);
        if window == protocol::WINDOW_TYPE_HORSE {
            ledger.apply(&content(
                ContainerIdentity::window(3),
                vec![NetworkItemStack::default(); 2],
            ));
        }
        let source = ledger.displayed_stack(SOURCE_SLOT).unwrap().clone();
        assert_quick_move(&mut ledger, &entry, old_target);
        assert_eq!(ledger.pending_batch().unwrap().unwrap().1, 1);

        let replacement = tagged_entry(&entry.identifier, after, None);
        resend(&mut ledger, &replacement);
        assert_eq!(ledger.pending_request_count(), 0);
        assert!(ledger.pending_batch().unwrap().is_none());
        assert_eq!(ledger.displayed_stack(SOURCE_SLOT), Some(&source));
        assert!(ledger.target_stack(old_target).is_none());

        assert_quick_move(&mut ledger, &replacement, new_target);
    }
}

#[test]
fn a_role_change_after_transport_requires_authoritative_recovery() {
    let entry = tagged_entry("custom:ruby_ingot", "minecraft:trim_materials", None);
    let mut ledger = ledger(&entry, 0);
    open(&mut ledger, protocol::WINDOW_TYPE_SMITHING_TABLE);
    assert_quick_move(&mut ledger, &entry, InventoryTarget::Craft(52));
    assert!(ledger.mark_transport_enqueued(10));

    let replacement = tagged_entry(&entry.identifier, "minecraft:trim_templates", None);
    resend(&mut ledger, &replacement);
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(ledger.pending_batch().unwrap().is_none());
    assert!(ledger.target_stack(InventoryTarget::Craft(52)).is_none());
    assert!(ledger.resync_required());
    assert_eq!(
        ledger.begin_quick_move(InventoryTarget::Player(SOURCE_SLOT)),
        Err(InventoryGestureError::ResyncRequired)
    );
}

#[test]
fn registry_tag_changes_keep_pending_moves_when_the_destination_is_unchanged() {
    let entry = tagged_entry("custom:ruby_ingot", "minecraft:trim_materials", None);
    let mut ledger = ledger(&entry, 0);
    open(&mut ledger, protocol::WINDOW_TYPE_SMITHING_TABLE);
    assert_quick_move(&mut ledger, &entry, InventoryTarget::Craft(52));
    let request_id = ledger.pending_request_id().unwrap();
    let actions = ledger.newest_request().unwrap().actions.clone();
    let predicted = ledger
        .target_stack(InventoryTarget::Craft(52))
        .unwrap()
        .clone();

    let replacement = tagged_entry(&entry.identifier, "minecraft:transform_materials", None);
    resend(&mut ledger, &replacement);
    assert_eq!(ledger.pending_request_count(), 1);
    assert_eq!(ledger.pending_request_id(), Some(request_id));
    assert_eq!(ledger.newest_request().unwrap().actions, actions);
    assert_eq!(ledger.pending_batch().unwrap().unwrap().1, 1);
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(52)),
        Some(&predicted)
    );
    assert!(ledger.displayed_stack(SOURCE_SLOT).is_none());
}

#[test]
fn component_items_do_not_inherit_brewing_or_beacon_roles_from_stock_names() {
    for (identifier, window) in [
        (
            "minecraft:blaze_powder",
            protocol::WINDOW_TYPE_BREWING_STAND,
        ),
        ("minecraft:diamond", protocol::WINDOW_TYPE_BEACON),
    ] {
        let mut entry = registered(identifier);
        entry.component_based = true;
        entry.version = protocol::ItemRegistryVersion::DataDriven;
        entry.components = Some(std::sync::Arc::new(protocol::ItemComponents::default()));
        let mut ledger = ledger(&entry, 0);
        open(&mut ledger, window);
        if window == protocol::WINDOW_TYPE_BREWING_STAND {
            ledger.apply(&content(
                ContainerIdentity::window(3),
                vec![NetworkItemStack::empty(); 5],
            ));
            assert_quick_move(&mut ledger, &entry, InventoryTarget::Storage(0));
        } else {
            assert_quick_move(&mut ledger, &entry, InventoryTarget::Player(0));
        }
    }
}

/// Opens a station with the content length declared by the protocol policy.
fn station_ledger(entry: &ItemRegistryEntry, window_type: i8) -> PlayerInventoryLedger {
    let mut ledger = ledger(entry, 0);
    open(&mut ledger, window_type);
    if let Some(&length) = ledger.window_kind().unwrap().content_lengths().first() {
        ledger.apply(&content(
            ContainerIdentity::window(3),
            vec![NetworkItemStack::empty(); length],
        ));
    }
    ledger
}

/// Checks rollback and recovery when the same registered identity gains or loses a role.
fn assert_station_route_refresh(
    identifier: &str,
    window_type: i8,
    native_target: InventoryTarget,
    component_target: InventoryTarget,
) {
    let native = registered(identifier);
    let mut component = native.clone();
    component.component_based = true;
    component.version = protocol::ItemRegistryVersion::DataDriven;
    component.components = Some(std::sync::Arc::new(protocol::ItemComponents::default()));
    for restore_native in [false, true] {
        for transported in [false, true] {
            let (entry, replacement, old_target, new_target) = if restore_native {
                (&component, &native, component_target, native_target)
            } else {
                (&native, &component, native_target, component_target)
            };
            let mut ledger = station_ledger(entry, window_type);
            let source = ledger.displayed_stack(SOURCE_SLOT).unwrap().clone();
            assert_quick_move(&mut ledger, entry, old_target);
            assert!(ledger.pending_batch().unwrap().is_some());
            if transported {
                assert!(ledger.mark_transport_enqueued(10));
            }

            resend(&mut ledger, replacement);
            assert_eq!(ledger.pending_request_count(), 0, "{identifier}");
            assert!(ledger.pending_batch().unwrap().is_none());
            assert!(ledger.target_stack(old_target).is_none());
            if transported {
                assert!(ledger.resync_required());
                assert_eq!(
                    ledger.begin_quick_move(InventoryTarget::Player(SOURCE_SLOT)),
                    Err(InventoryGestureError::ResyncRequired),
                );
            } else {
                assert_eq!(ledger.displayed_stack(SOURCE_SLOT), Some(&source));
                assert!(!ledger.resync_required());
                assert_quick_move(&mut ledger, replacement, new_target);
            }
        }
    }
}

#[test]
fn fact_dependent_beacon_moves_retire_when_registry_routes_change() {
    assert_station_route_refresh(
        "minecraft:diamond",
        protocol::WINDOW_TYPE_BEACON,
        InventoryTarget::Craft(27),
        InventoryTarget::Player(0),
    );
}

#[test]
fn fact_dependent_brewing_moves_retire_when_registry_routes_change() {
    for (identifier, native_slot) in [("minecraft:blaze_powder", 4), ("minecraft:potion", 1)] {
        assert_station_route_refresh(
            identifier,
            protocol::WINDOW_TYPE_BREWING_STAND,
            InventoryTarget::Storage(native_slot),
            InventoryTarget::Storage(0),
        );
    }
}

#[test]
fn fact_dependent_enchanting_moves_retire_when_registry_routes_change() {
    assert_station_route_refresh(
        "minecraft:lapis_lazuli",
        protocol::WINDOW_TYPE_ENCHANTMENT,
        InventoryTarget::Craft(15),
        InventoryTarget::Craft(14),
    );
}

#[test]
fn fact_dependent_moves_keep_requests_when_destinations_do_not_change() {
    for (identifier, window, target) in [
        (
            "minecraft:diamond",
            protocol::WINDOW_TYPE_BEACON,
            InventoryTarget::Craft(27),
        ),
        (
            "minecraft:diamond",
            protocol::WINDOW_TYPE_ENCHANTMENT,
            InventoryTarget::Craft(14),
        ),
        (
            "minecraft:brick",
            protocol::WINDOW_TYPE_BREWING_STAND,
            InventoryTarget::Storage(0),
        ),
        (
            "minecraft:potion",
            protocol::WINDOW_TYPE_BREWING_STAND,
            InventoryTarget::Storage(1),
        ),
    ] {
        let entry = registered(identifier);
        let mut ledger = station_ledger(&entry, window);
        assert_quick_move(&mut ledger, &entry, target);
        let request_id = ledger.pending_request_id().unwrap();
        let actions = ledger.newest_request().unwrap().actions.clone();
        let predicted = ledger.target_stack(target).unwrap().clone();
        let mut replacement = entry.clone();
        replacement.item_tags = [std::sync::Arc::from("minecraft:is_sword")].into();

        resend(&mut ledger, &replacement);
        assert!(
            ledger
                .item_facts()
                .get(entry.network_id)
                .traits
                .contains(assets::facts::ItemTraits::SWORD)
        );
        assert_eq!(ledger.pending_request_count(), 1);
        assert_eq!(ledger.pending_request_id(), Some(request_id));
        assert_eq!(ledger.newest_request().unwrap().actions, actions);
        assert_eq!(ledger.target_stack(target), Some(&predicted));
        assert!(ledger.pending_batch().unwrap().is_some());
    }
}

/// Checks admission for manually placing an unrelated item into a station input.
#[test]
fn parity_actions_manual_station_placement_rejects_foreign_items() {
    for (window, target) in [
        (
            protocol::WINDOW_TYPE_ENCHANTMENT,
            InventoryTarget::Craft(15),
        ),
        (protocol::WINDOW_TYPE_BEACON, InventoryTarget::Craft(27)),
    ] {
        let entry = registered("minecraft:dirt");
        let mut ledger = ledger(&entry, 0);
        open(&mut ledger, window);
        ledger.begin_click(SOURCE_SLOT).unwrap();
        let queued = ledger.pending_request_count();
        let result = ledger.begin_target_gesture(target, CellGesture::Click);
        assert!(
            result.is_err(),
            "foreign manual placement admitted for {target:?}: {result:?}"
        );
        assert_eq!(ledger.pending_request_count(), queued);
    }
}

/// Checks the one-item limit on a manually filled enchanting input.
#[test]
fn parity_actions_manual_enchant_input_has_one_item_limit() {
    let entry = registered("minecraft:dirt");
    let mut ledger = ledger(&entry, 0);
    open(&mut ledger, protocol::WINDOW_TYPE_ENCHANTMENT);
    let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[usize::from(SOURCE_SLOT)] = NetworkItemStack {
        network_id: entry.network_id,
        stack_network_id: 100,
        count: 5,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&content(ContainerIdentity::window(0), slots));
    ledger.begin_click(SOURCE_SLOT).unwrap();
    ledger
        .begin_target_gesture(InventoryTarget::Craft(14), CellGesture::Click)
        .unwrap();
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(14))
            .unwrap()
            .count,
        1
    );
    assert_eq!(ledger.cursor_stack().unwrap().count, 4);
}

/// Checks whether local station close emits a return for an occupied station input.
#[test]
fn parity_actions_close_returns_enchanting_input() {
    let entry = registered("minecraft:diamond_sword");
    let mut ledger = ledger(&entry, 0);
    open(&mut ledger, protocol::WINDOW_TYPE_ENCHANTMENT);
    let mut slots = vec![NetworkItemStack::empty(); protocol::UI_SLOT_COUNT];
    slots[14] = NetworkItemStack {
        network_id: entry.network_id,
        stack_network_id: 101,
        count: 1,
        ..NetworkItemStack::empty()
    };
    ledger.apply(&content(
        ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
        slots,
    ));
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(14))
            .unwrap()
            .count,
        1
    );
    ledger.request_storage_close();
    assert!(
        ledger.newest_request().is_some(),
        "close must return the occupied station input"
    );
    assert!(ledger.target_stack(InventoryTarget::Craft(14)).is_none());
}

#[path = "quick_move_role_tests/station_inputs.rs"]
mod station_inputs;
