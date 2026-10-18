//! Chest returns use the combined player inventory's slot order.

use super::*;

/// Opens a chest with an identified source stack and known player slots.
fn chest(slots: &[(usize, NetworkItemStack)], source: NetworkItemStack) -> PlayerInventoryLedger {
    let mut ledger = known_ledger(slots);
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    let mut storage = vec![NetworkItemStack::default(); SMALL_STORAGE_SLOT_COUNT];
    storage[2] = source;
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(7),
            slot_type: Some(CONTAINER_NAME_LEVEL_ENTITY),
            dynamic_id: None,
        },
        slots: storage.into(),
        storage_item: NetworkItemStack::default(),
    }));
    ledger
}

/// Reads the actual ordered request amounts and destination slots.
fn placements(ledger: &PlayerInventoryLedger) -> Vec<(u8, u8)> {
    ledger
        .newest_request()
        .unwrap()
        .actions
        .iter()
        .map(|action| {
            let StackRequestAction::Place {
                amount,
                source,
                destination,
            } = action
            else {
                panic!("quick moves place directly without using the cursor");
            };
            assert_eq!(
                source.container,
                StackRequestContainer::LevelEntity { dynamic_id: None }
            );
            assert_eq!(source.slot, 2);
            assert_eq!(source.stack_network_id, 100);
            assert_eq!(
                destination.container,
                StackRequestContainer::PlayerInventory
            );
            (*amount, destination.slot)
        })
        .collect()
}

#[test]
fn chest_return_fills_first_available_hotbar_cell() {
    let mut ledger = chest(
        &[
            (0, stack(8, 10, 1)),
            (1, stack(8, 11, 1)),
            (2, stack(8, 12, 1)),
        ],
        stack(6, 100, 10),
    );
    ledger
        .begin_quick_move(InventoryTarget::Storage(2))
        .unwrap();
    assert_eq!(placements(&ledger), [(10, 3)]);
    assert_eq!(ledger.displayed_stack(3).unwrap().count, 10);
    assert!(
        ledger
            .displayed_stack(protocol::HOTBAR_SLOT_COUNT)
            .is_none()
    );
    assert!(ledger.storage_stack(2).is_none());
    assert!(ledger.cursor_stack().is_none());
    assert!(ledger.pending_batch().unwrap().is_some());
}

#[test]
fn chest_return_merges_across_both_sections_before_using_empty_hotbar() {
    let main = protocol::HOTBAR_SLOT_COUNT + 2;
    let mut ledger = chest(
        &[(5, stack(6, 10, 62)), (main.into(), stack(6, 11, 61))],
        stack(6, 100, 10),
    );
    ledger
        .begin_quick_move(InventoryTarget::Storage(2))
        .unwrap();
    assert_eq!(placements(&ledger), [(2, 5), (3, main), (5, 0)]);
    assert_eq!(ledger.displayed_stack(5).unwrap().count, 64);
    assert_eq!(ledger.displayed_stack(main).unwrap().count, 64);
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 5);
}

#[test]
fn chest_return_skips_ineligible_merges_before_placing_in_hotbar() {
    let mut named = stack(6, 11, 60);
    named.metadata = 1;
    let mut ledger = chest(&[(0, stack(6, 10, 60)), (1, named)], stack(6, 100, 10));
    ledger.set_confirmed_overlay(
        Cell::Inventory(0),
        StackResponseOverlay {
            custom_name: Some(Arc::from("Named apple")),
            ..Default::default()
        },
    );
    ledger
        .begin_quick_move(InventoryTarget::Storage(2))
        .unwrap();
    assert_eq!(placements(&ledger), [(10, 2)]);
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 60);
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 60);
}

#[test]
fn chest_return_traverses_hotbar_then_main_and_splits_at_capacity() {
    let last_hotbar = protocol::HOTBAR_SLOT_COUNT - 1;
    let first_main = protocol::HOTBAR_SLOT_COUNT;
    let occupied: Vec<_> = (0..last_hotbar)
        .map(|slot| (usize::from(slot), stack(8, 10 + i32::from(slot), 1)))
        .collect();
    let mut ledger = chest(&occupied, stack(6, 100, 130));
    ledger
        .begin_quick_move(InventoryTarget::Storage(2))
        .unwrap();
    assert_eq!(
        placements(&ledger),
        [(64, last_hotbar), (64, first_main), (2, first_main + 1)]
    );
    assert!(ledger.storage_stack(2).is_none());
}

#[test]
fn chest_return_with_full_hotbar_uses_first_available_main_cell() {
    let first_main = protocol::HOTBAR_SLOT_COUNT;
    let occupied: Vec<_> = (0..=first_main)
        .map(|slot| (usize::from(slot), stack(8, 10 + i32::from(slot), 1)))
        .collect();
    let mut ledger = chest(&occupied, stack(6, 100, 10));
    ledger
        .begin_quick_move(InventoryTarget::Storage(2))
        .unwrap();
    assert_eq!(placements(&ledger), [(10, first_main + 1)]);
}

#[test]
fn chest_return_keeps_unplaced_remainder_when_player_inventory_is_full() {
    let occupied: Vec<_> = (0..protocol::PLAYER_INVENTORY_SLOTS)
        .map(|slot| (usize::from(slot), stack(6, 10 + i32::from(slot), 64)))
        .collect();
    let mut ledger = chest(&occupied, stack(6, 100, 10));
    assert_eq!(
        ledger.begin_quick_move(InventoryTarget::Storage(2)),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.pending_request_count(), 0);
    assert_eq!(ledger.storage_stack(2), Some(&stack(6, 100, 10)));
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 4,
        },
        stack: stack(6, 14, 62),
        storage_item: None,
    }));
    ledger
        .begin_quick_move(InventoryTarget::Storage(2))
        .unwrap();
    assert_eq!(placements(&ledger), [(2, 4)]);
    assert_eq!(ledger.storage_stack(2).unwrap().count, 8);
}

#[test]
fn rejected_chest_return_restores_merges_empty_cells_and_source() {
    let main = protocol::HOTBAR_SLOT_COUNT;
    let before = [(5, stack(6, 10, 62)), (usize::from(main), stack(6, 11, 61))];
    let source = stack(6, 100, 130);
    let mut ledger = chest(&before, source.clone());
    let request = ledger
        .begin_quick_move(InventoryTarget::Storage(2))
        .unwrap();
    assert_eq!(placements(&ledger), [(2, 5), (3, main), (64, 0), (61, 1)]);
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Response(
        protocol::ItemStackResponseEvent {
            responses: Arc::from([protocol::StackResponse {
                status: protocol::StackResponseStatus::Rejected,
                request_id: request,
                containers: Arc::from([]),
            }]),
        },
    ));
    for (slot, expected) in before {
        assert_eq!(ledger.displayed_stack(slot as u8), Some(&expected));
    }
    assert!(ledger.displayed_stack(0).is_none());
    assert!(ledger.displayed_stack(1).is_none());
    assert_eq!(ledger.storage_stack(2), Some(&source));
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(ledger.cursor_stack().is_none());
}
