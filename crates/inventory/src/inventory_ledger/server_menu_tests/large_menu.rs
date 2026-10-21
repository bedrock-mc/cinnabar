use super::*;

#[test]
fn server_menu_200_slots_retain_high_cells_without_requesting_close() {
    let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
    let container = descriptor(Some(0), None);
    let mut slots = vec![NetworkItemStack::empty(); 200];
    slots[113] = stack(113);
    slots[199] = stack(199);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container,
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));

    assert_eq!(ledger.storage_slot_count(), Some(200));
    assert!(ledger.pending_batch().unwrap().is_none());
    assert_eq!(ledger.storage_stack(113), Some(&stack(113)));
    assert_eq!(ledger.storage_stack(199), Some(&stack(199)));

    ledger.begin_storage_click(199).unwrap();
    let Some(StackRequestAction::Take { source, .. }) = ledger.newest_action() else {
        panic!("taking the last server menu item");
    };
    assert_eq!(source.slot, 199);
    assert_eq!(source.stack_network_id, 199);
}
