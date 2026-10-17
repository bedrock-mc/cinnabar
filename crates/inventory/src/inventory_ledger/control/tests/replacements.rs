use super::*;

#[test]
fn rejected_replacement_preserves_storage_then_allows_a_personal_open() {
    for replacement_id in [3, 4] {
        for expire_close in [false, true] {
            let mut ledger = ledger_with_slot_zero();
            let mut server = Server::default();
            server.open_storage(&mut ledger, 3);
            let original = ledger.controls.storage.confirmed;
            let generation = ledger.storage_generation();
            let InventoryEvent::Open(mut replacement) = storage_open(replacement_id) else {
                unreachable!()
            };
            replacement.window_type = i8::MAX;
            assert!(protocol::WindowKind::from_window_type(replacement.window_type).is_none());
            ledger.apply(&InventoryEvent::Open(replacement));

            assert_eq!(ledger.storage_generation(), generation);
            assert_eq!(ledger.controls.storage.confirmed, original);
            assert!(ledger.controls.storage.desired_open);
            if replacement_id != 3 {
                assert_eq!(
                    ledger.pending_control_packet().unwrap().unwrap(),
                    protocol::container_close_packet(replacement_id).unwrap()
                );
                server.send(&mut ledger, 0, expire_close);
                if expire_close {
                    ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
                } else {
                    server.deliver(&mut ledger);
                }
            }
            assert_eq!(ledger.storage_generation(), generation);
            assert_eq!(ledger.controls.storage.confirmed, original);
            ledger.request_storage_close();
            let mut now = INVENTORY_REQUEST_TIMEOUT_MILLIS;
            server.settle(&mut ledger, &mut now);
            assert!(ledger.request_personal_open(42));
            server.settle(&mut ledger, &mut now);
            assert!(server.personal_open);
            assert!(ledger.begin_click(0).is_ok());
        }
    }
}

#[test]
fn rejected_replacement_preserves_storage_contents_and_pending_items() {
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
