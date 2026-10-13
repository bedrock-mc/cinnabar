//! Fixed player windows beyond the 36 cells: vanilla request names, offhand
//! wire slot 1, armor, and the personal crafting cells.

use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_ARMOR, CONTAINER_NAME_CRAFT_INPUT, CONTAINER_NAME_CURSOR,
    CONTAINER_NAME_INVENTORY, CONTAINER_NAME_OFFHAND, ContainerIdentity, ContainerOpenEvent,
    InventoryContentEvent, InventoryEvent, InventorySlotEvent, ItemStackResponseEvent,
    NetworkItemStack, OFFHAND_WINDOW_ID, SlotIdentity, StackRequestAction, StackRequestContainer,
    StackResponse, StackResponseContainer, StackResponseSlot, StackResponseStatus,
};

use super::*;

fn stack(stack_network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id: 6,
        count,
        stack_network_id,
        ..NetworkItemStack::default()
    }
}

fn content(container: ContainerIdentity, slots: Vec<NetworkItemStack>) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: Arc::from(slots),
        storage_item: NetworkItemStack::default(),
    })
}

fn named(slot_type: u8) -> ContainerIdentity {
    ContainerIdentity {
        window_id: None,
        slot_type: Some(slot_type),
        dynamic_id: None,
    }
}

fn open_ledger(slot: usize, held: NetworkItemStack) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    let mut slots = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    slots[slot] = held;
    ledger.apply(&content(ContainerIdentity::window(0), slots));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger
}

fn accept(ledger: &mut PlayerInventoryLedger, request_id: i32, rows: &[(u8, u8, u8, i32)]) {
    let containers: Vec<StackResponseContainer> = rows
        .iter()
        .map(|(name, slot, count, id)| StackResponseContainer {
            container: named(*name),
            slots: Arc::from([StackResponseSlot {
                slot: *slot,
                hotbar_slot: *slot,
                count: *count,
                item_stack_id: *id,
                custom_name: Arc::from(""),
                filtered_custom_name: Arc::from(""),
                durability_correction: 0,
            }]),
        })
        .collect();
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from(containers),
        }]),
    }));
}

/// The captured take response names the inventory container without a window;
/// it must correct the player cell rather than be skipped.
#[test]
fn captured_take_response_corrects_inventory_and_cursor_cells() {
    let mut ledger = open_ledger(13, stack(358, 1));
    let request = ledger.begin_click(13).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    accept(
        &mut ledger,
        request,
        &[
            (CONTAINER_NAME_INVENTORY, 13, 0, 0),
            (CONTAINER_NAME_CURSOR, 0, 1, 900),
        ],
    );
    assert_eq!(ledger.skipped_unknown_containers(), 0);
    assert!(ledger.displayed_stack(13).is_none());
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 900);
    assert!(!ledger.resync_required());
}

/// Registers the stack helper's item as the stock item `identifier`.
fn registry(identifier: &str) -> protocol::ItemRegistryEvent {
    protocol::ItemRegistryEvent {
        entries: Arc::from([protocol::ItemRegistryEntry {
            identifier: Arc::from(identifier),
            network_id: 6,
            component_based: false,
            version: protocol::ItemRegistryVersion::None,
            components: None,
            component_digest: [0; 32],
            negotiated_max_stack_size: None,
            canonical_empty_component_data: true,
            item_tags: Arc::from([]),
        }]),
    }
}

/// Offhand content lands at index 0, requests address wire slot 1, and the
/// response echo of slot 1 reconciles the same cell.
#[test]
fn offhand_place_uses_wire_slot_one_end_to_end() {
    let mut ledger = open_ledger(14, stack(493, 1));
    ledger.apply_registry(&registry("minecraft:shield"));
    ledger.apply(&content(
        ContainerIdentity::window(OFFHAND_WINDOW_ID),
        vec![NetworkItemStack::default()],
    ));
    ledger.begin_click(14).unwrap();
    let place = ledger
        .begin_target_gesture(InventoryTarget::Offhand, CellGesture::Click)
        .unwrap();
    let Some(StackRequestAction::Place { destination, .. }) = ledger.newest_action() else {
        panic!("placing into an empty offhand");
    };
    assert_eq!(destination.container, StackRequestContainer::Offhand);
    assert_eq!(destination.slot, 1);
    assert_eq!(
        ledger.target_stack(InventoryTarget::Offhand).unwrap().count,
        1
    );

    while ledger.pending_batch().unwrap().is_some() {
        assert!(ledger.mark_transport_enqueued(10));
    }
    let take = ledger.pending_request_id().unwrap();
    accept(&mut ledger, take, &[]);
    accept(&mut ledger, place, &[(CONTAINER_NAME_OFFHAND, 1, 1, 494)]);
    let offhand = ledger.target_stack(InventoryTarget::Offhand).unwrap();
    assert_eq!(offhand.stack_network_id, 494);
    assert!(ledger.cursor_stack().is_none());
    assert!(!ledger.resync_required());
}

/// Opens a personal screen holding `count` of `identifier` in slot 14, with
/// empty armor and offhand cells.
fn equipment_ledger(identifier: &str, count: u16) -> PlayerInventoryLedger {
    let mut ledger = open_ledger(14, stack(493, count));
    ledger.apply_registry(&registry(identifier));
    ledger.apply(&content(
        ContainerIdentity::window(OFFHAND_WINDOW_ID),
        vec![NetworkItemStack::default()],
    ));
    ledger.apply(&content(
        ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
        vec![NetworkItemStack::default(); 4],
    ));
    ledger
}

#[test]
fn equipment_cells_refuse_items_they_do_not_admit() {
    let mut ledger = equipment_ledger("minecraft:dirt", 5);
    ledger.begin_click(14).unwrap();
    let queued = ledger.pending_request_count();
    for target in [InventoryTarget::Offhand, InventoryTarget::Armor(0)] {
        for gesture in [CellGesture::Click, CellGesture::PlaceCount(1)] {
            assert_eq!(
                ledger.begin_target_gesture(target, gesture),
                Err(InventoryGestureError::EquipmentRefused)
            );
        }
    }
    assert_eq!(ledger.pending_request_count(), queued);
    assert_eq!(ledger.cursor_stack().unwrap().count, 5);
    assert!(ledger.target_stack(InventoryTarget::Offhand).is_none());
    assert!(ledger.target_stack(InventoryTarget::Armor(0)).is_none());
}

#[test]
fn hotbar_key_refuses_moving_a_foreign_item_into_armor() {
    let mut ledger = open_ledger(4, stack(493, 5));
    ledger.apply_registry(&registry("minecraft:dirt"));
    ledger.apply(&content(
        ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
        vec![NetworkItemStack::default(); 4],
    ));
    assert_eq!(
        ledger.begin_hotbar_swap(InventoryTarget::Armor(3), 4),
        Err(InventoryGestureError::EquipmentRefused)
    );
    assert_eq!(ledger.displayed_stack(4).unwrap().count, 5);
}

#[test]
fn foreign_items_already_in_equipment_can_be_taken_out() {
    let mut ledger = equipment_ledger("minecraft:dirt", 5);
    ledger.apply(&content(
        ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
        vec![
            stack(600, 1),
            NetworkItemStack::default(),
            NetworkItemStack::default(),
            NetworkItemStack::default(),
        ],
    ));
    ledger
        .begin_target_gesture(InventoryTarget::Armor(0), CellGesture::Click)
        .unwrap();
    assert!(ledger.target_stack(InventoryTarget::Armor(0)).is_none());
    let Some(StackRequestAction::Take { source, .. }) = ledger.newest_action() else {
        panic!("taking out of the helmet cell");
    };
    assert_eq!(source.stack_network_id, 600);
    assert!(ledger.cursor_stack().is_some());
}

#[test]
fn admitted_head_items_fill_the_helmet_cell_one_at_a_time() {
    let mut ledger = equipment_ledger("minecraft:carved_pumpkin", 5);
    ledger.begin_click(14).unwrap();
    ledger
        .begin_target_gesture(InventoryTarget::Armor(0), CellGesture::Click)
        .unwrap();
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Armor(0))
            .unwrap()
            .count,
        1
    );
    assert_eq!(ledger.cursor_stack().unwrap().count, 4);
    assert_eq!(
        ledger.begin_target_gesture(InventoryTarget::Armor(1), CellGesture::Click),
        Err(InventoryGestureError::EquipmentRefused)
    );
}

/// Armor content and slot updates are retained, including the body slot.
#[test]
fn armor_content_and_slot_updates_are_retained() {
    let mut ledger = open_ledger(0, NetworkItemStack::default());
    ledger.apply(&content(
        ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
        vec![
            stack(1, 1),
            NetworkItemStack::default(),
            stack(3, 1),
            stack(4, 1),
        ],
    ));
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Armor(0))
            .unwrap()
            .stack_network_id,
        1
    );
    assert!(ledger.target_stack(InventoryTarget::Armor(1)).is_none());
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: named(CONTAINER_NAME_ARMOR),
            slot: 4,
        },
        stack: stack(5, 1),
        storage_item: None,
    }));
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Armor(4))
            .unwrap()
            .stack_network_id,
        5
    );
    assert_eq!(ledger.skipped_unknown_containers(), 0);
}

#[test]
fn gear_observation_distinguishes_unknown_empty_and_resets_with_the_session() {
    let mut ledger = PlayerInventoryLedger::default();
    let helmet = InventoryTarget::Armor(0);
    assert_eq!(
        ledger.gear_slot_state(helmet),
        Some(PlayerInventorySlot::Unknown)
    );
    assert_eq!(
        ledger.gear_slot_state(InventoryTarget::Offhand),
        Some(PlayerInventorySlot::Unknown)
    );
    ledger.apply(&content(
        ContainerIdentity::window(protocol::ARMOR_WINDOW_ID),
        vec![NetworkItemStack::empty()],
    ));
    assert_eq!(
        ledger.gear_slot_state(helmet),
        Some(PlayerInventorySlot::Empty)
    );
    assert_eq!(
        ledger.gear_slot_state(InventoryTarget::Armor(1)),
        Some(PlayerInventorySlot::Unknown)
    );
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: named(CONTAINER_NAME_OFFHAND),
            slot: 0,
        },
        stack: stack(1, 8),
        storage_item: None,
    }));
    assert!(
        matches!(ledger.gear_slot_state(InventoryTarget::Offhand), Some(PlayerInventorySlot::Present(item)) if item.count == 8)
    );
    assert_eq!(
        ledger.gear_slot_state(InventoryTarget::Armor(u8::MAX)),
        None
    );
    assert_eq!(ledger.gear_slot_state(InventoryTarget::Player(0)), None);
    ledger.begin_session(2);
    assert_eq!(
        ledger.gear_slot_state(helmet),
        Some(PlayerInventorySlot::Unknown)
    );
    assert_eq!(
        ledger.gear_slot_state(InventoryTarget::Offhand),
        Some(PlayerInventorySlot::Unknown)
    );
}

/// Personal UI inventory content fills every crafting cell; named crafting
/// input updates address the same cells.
#[test]
fn ui_inventory_content_and_named_updates_fill_crafting_cells() {
    let mut ledger = open_ledger(0, NetworkItemStack::default());
    let mut ui = vec![NetworkItemStack::default(); 54];
    ui[28] = stack(10, 1);
    ui[40] = stack(11, 1);
    ledger.apply(&content(
        ContainerIdentity {
            window_id: Some(124),
            slot_type: Some(0),
            dynamic_id: None,
        },
        ui,
    ));
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(28))
            .unwrap()
            .stack_network_id,
        10
    );
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(40))
            .unwrap()
            .stack_network_id,
        11
    );
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity {
                window_id: Some(124),
                slot_type: Some(CONTAINER_NAME_CRAFT_INPUT),
                dynamic_id: None,
            },
            slot: 29,
        },
        stack: stack(12, 2),
        storage_item: None,
    }));
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(29))
            .unwrap()
            .count,
        2
    );
    assert_eq!(ledger.skipped_unknown_containers(), 0);
}

/// Geyser sets and clears the cursor with a slot update for slot 0 of the
/// personal UI inventory under the default name.
#[test]
fn personal_ui_slot_zero_update_sets_and_clears_the_cursor() {
    let mut ledger = open_ledger(0, NetworkItemStack::default());
    let cursor = |stack| {
        InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(124),
                    slot_type: Some(0),
                    dynamic_id: None,
                },
                slot: 0,
            },
            stack,
            storage_item: None,
        })
    };
    ledger.apply(&cursor(stack(7, 1)));
    assert_eq!(
        ledger.cursor_stack().map(|held| held.stack_network_id),
        Some(7)
    );
    ledger.apply(&cursor(NetworkItemStack::default()));
    assert_eq!(ledger.cursor_stack(), None);
    assert_eq!(ledger.skipped_unknown_containers(), 0);
}

/// Full UI snapshots replace the cursor alias while retaining crafting-slot routing.
#[test]
fn review_full_ui_snapshot_replaces_aliased_cursor() {
    for count in [0, 2] {
        let mut ledger = open_ledger(0, NetworkItemStack::default());
        let identity = ContainerIdentity {
            window_id: Some(protocol::UI_INVENTORY_WINDOW_ID),
            slot_type: Some(0),
            dynamic_id: None,
        };
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: identity,
                slot: 0,
            },
            stack: stack(7, 1),
            storage_item: None,
        }));
        let mut slots = vec![NetworkItemStack::default(); protocol::UI_SLOT_COUNT];
        slots[0] = stack(8, count);
        slots[28] = stack(9, 1);
        ledger.apply(&content(identity, slots));
        assert_eq!(
            ledger.cursor_stack().map(|held| held.count),
            (count > 0).then_some(count)
        );
        assert_eq!(
            ledger
                .target_stack(InventoryTarget::Craft(28))
                .unwrap()
                .stack_network_id,
            9
        );
        assert_eq!(ledger.skipped_unknown_containers(), 0);
    }
}
