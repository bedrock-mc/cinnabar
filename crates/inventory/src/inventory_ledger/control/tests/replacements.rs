use super::*;

#[test]
fn unsupported_replacement_preserves_storage_and_does_not_send_a_close() {
    for replacement_id in [3, 4] {
        let mut ledger = ledger_with_slot_zero();
        ledger.apply(&storage_open(3));
        let generation = ledger.storage_generation();
        let InventoryEvent::Open(mut replacement) = storage_open(replacement_id) else {
            unreachable!()
        };
        replacement.window_type = i8::MAX;
        assert!(protocol::WindowKind::from_window_type(replacement.window_type).is_none());
        ledger.apply(&InventoryEvent::Open(replacement));
        assert_eq!(ledger.storage_generation(), generation);
        assert_eq!(ledger.skipped_unknown_containers(), 1);
        assert!(ledger.pending_batch().unwrap().is_none());
        ledger.request_storage_close();
        assert!(ledger.mark_transport_enqueued(0));
        acknowledge_close(&mut ledger, 3, GENERIC_STORAGE_WINDOW_TYPE);
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(1));
        ledger.apply(&InventoryEvent::Open(personal_open(7)));
        assert!(ledger.begin_click(0).is_ok());
    }
}

#[test]
fn unsupported_replacement_preserves_storage_contents_and_pending_items() {
    let mut ledger = ledger_with_slot_zero();
    ledger.apply(&storage_open(3));
    let count = *protocol::WindowKind::Storage
        .content_lengths()
        .iter()
        .min()
        .unwrap();
    let mut slots = vec![protocol::NetworkItemStack::empty(); count];
    slots[0] = protocol::NetworkItemStack {
        network_id: 6,
        stack_network_id: 13,
        count: 3,
        ..Default::default()
    };
    ledger.apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
        container: ContainerIdentity::window(3),
        slots: std::sync::Arc::from(slots),
        storage_item: protocol::NetworkItemStack::empty(),
    }));
    let request = ledger.begin_storage_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(0));
    let cursor = ledger.cursor_stack().cloned();
    let contents = ledger.confirmed.clone();
    let original = ledger.controls.storage.confirmed;
    let InventoryEvent::Open(mut replacement) = storage_open(4) else {
        unreachable!()
    };
    replacement.window_type = i8::MAX;
    ledger.apply(&InventoryEvent::Open(replacement));

    assert_eq!(ledger.controls.storage.confirmed, original);
    assert_eq!(ledger.pending_request_id(), Some(request));
    assert_eq!(ledger.cursor_stack(), cursor.as_ref());
    assert_eq!(ledger.confirmed.storage_len(), contents.storage_len());
    for slot in 0..contents.storage_len() {
        let cell = crate::inventory_ledger::Cell::Storage(slot as u8);
        assert_eq!(ledger.confirmed.get(cell), contents.get(cell));
    }
}
