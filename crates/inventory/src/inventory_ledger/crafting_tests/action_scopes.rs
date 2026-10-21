//! Manual and repeated crafting request scopes.

use super::*;

/// Checks that another output take can merge into a compatible held stack.
#[test]
fn parity_actions_manual_output_merges_into_cursor() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    ledger.apply(&craft_slot(28, stack(LOG, 101, 2)));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: None,
            slot_type: Some(CONTAINER_NAME_CURSOR),
            dynamic_id: None,
        },
        slots: Arc::from([stack(PLANKS, 202, 3)]),
        storage_item: NetworkItemStack::empty(),
    }));
    let recipe = unique(&ledger, &catalog());
    let result = ledger.begin_craft_into(&recipe, 1, CraftSink::Cursor);
    assert!(result.is_ok(), "compatible cursor has room: {result:?}");
    assert_eq!(ledger.cursor_stack().unwrap().count, 4);
}

/// Checks that shift crafting fills all three available output spaces.
#[test]
fn parity_actions_craft_all_uses_maximum_fitting_count() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    let mut slots: Vec<_> = (0..PLAYER_INVENTORY_SLOT_COUNT)
        .map(|slot| stack(COBBLE, 300 + slot as i32, 64))
        .collect();
    slots[0] = stack(PLANKS, 250, 61);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
    ledger.apply(&craft_slot(28, stack(LOG, 101, 10)));
    let recipe = unique(&ledger, &catalog());
    ledger.begin_craft_all(&recipe).unwrap();
    assert_eq!(
        ledger.displayed_stack(0).unwrap().count,
        64,
        "exactly three crafts fit"
    );
    assert_eq!(
        ledger
            .target_stack(InventoryTarget::Craft(28))
            .unwrap()
            .count,
        7
    );
}

/// Checks close admission while a manual craft and its created output await a response.
#[test]
fn parity_actions_close_during_manual_craft() {
    let mut ledger = ledger(WORKBENCH_WINDOW_TYPE);
    ledger.apply(&craft_slot(32, stack(LOG, 101, 2)));
    let recipe = unique(&ledger, &catalog());
    ledger.begin_craft(&recipe, 1).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.request_storage_close();
    assert!(ledger.cursor_stack().is_none());
    assert!(ledger.target_stack(InventoryTarget::Craft(32)).is_none());
    assert_eq!(
        ledger.pending_request_count(),
        3,
        "craft plus input and cursor returns"
    );
    assert_eq!(
        ledger.begin_craft(&recipe, 1),
        Err(InventoryGestureError::ResyncRequired)
    );
    let (_, count) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(count, 2, "both returns share the next request batch");
    for _ in 0..count {
        assert!(ledger.mark_transport_enqueued(11));
    }
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(format!("{:?}", packet.kind()), "ContainerClosePacket");
    assert_eq!(
        ledger.pending_request_count(),
        3,
        "no answer was needed for close admission"
    );
}
