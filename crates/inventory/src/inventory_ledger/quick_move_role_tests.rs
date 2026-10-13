//! Quick-move requests for items from the pinned registry and role data.

use assets::item::armour::{ITEM_ARMOUR_SOURCE_BYTES, ItemArmour, ItemArmourSlot};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryContentEvent, InventoryEvent,
    ItemRegistryEntry, ItemRegistryEvent, NetworkItemStack, StackRequestAction,
    StackRequestContainer, WindowKind,
};
use serde::Deserialize;

use super::*;

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
            protocol::open_cell_request(WindowKind::Horse, slot, None, None).unwrap()
        }
        InventoryTarget::Player(slot) => (StackRequestContainer::PlayerInventory, slot),
        _ => panic!("unsupported quick-move fixture target"),
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
fn unrelated_session_tags_preserve_the_pinned_smithing_fallback() {
    let entry = tagged_entry("minecraft:netherite_ingot", "minecraft:is_food", None);
    let mut ledger = ledger(&entry, 0);
    open(&mut ledger, protocol::WINDOW_TYPE_SMITHING_TABLE);
    assert_quick_move(&mut ledger, &entry, InventoryTarget::Craft(52));
}

#[test]
fn negotiated_banner_tag_routes_a_custom_item_to_the_loom() {
    let entry = tagged_entry("custom:standard", "minecraft:banner", None);
    let mut ledger = ledger(&entry, 11);
    open(&mut ledger, protocol::WINDOW_TYPE_LOOM);
    assert_quick_move(&mut ledger, &entry, InventoryTarget::Craft(9));
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
