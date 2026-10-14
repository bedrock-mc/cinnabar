use super::*;

#[test]
fn repeated_chest_open_keeps_contents_and_pending_requests() {
    for pending in [false, true] {
        let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
        let menu = descriptor(Some(0), None);
        ledger.apply(&content(menu));
        let generation = ledger.storage_generation();
        let request = pending.then(|| {
            let request = ledger.begin_storage_click((MENU_CELLS - 1) as u8).unwrap();
            assert!(ledger.mark_transport_enqueued(0));
            request
        });
        for _ in 0..20 {
            ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
                container: ContainerIdentity::window(MENU_WINDOW),
                window_type: GENERIC_STORAGE_WINDOW_TYPE,
                server_initiated: false,
            }));
            ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(MENU_WINDOW),
                window_type: GENERIC_STORAGE_WINDOW_TYPE,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }));
            assert_eq!(ledger.storage_generation(), generation);
            assert_eq!(ledger.storage_slot_count(), Some(MENU_CELLS));
            assert_eq!(ledger.pending_request_id(), request);
            assert!(!ledger.resync_required());
        }
        if let Some(request_id) = request {
            ledger.apply(&InventoryEvent::Response(
                protocol::ItemStackResponseEvent {
                    responses: vec![protocol::StackResponse {
                        request_id,
                        status: protocol::StackResponseStatus::Rejected,
                        containers: Arc::from([]),
                    }]
                    .into(),
                },
            ));
        }
        assert_eq!(
            ledger.storage_stack((MENU_CELLS - 1) as u8),
            Some(&stack(100))
        );
        assert!(ledger.begin_storage_click((MENU_CELLS - 1) as u8).is_ok());
    }
}

#[test]
fn repeated_chest_open_does_not_claim_different_owners() {
    let original = ContainerOpenEvent {
        container: ContainerIdentity::window(MENU_WINDOW),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    };
    for replacement in [
        ContainerOpenEvent {
            container: ContainerIdentity::window(MENU_WINDOW + 1),
            ..original
        },
        ContainerOpenEvent {
            position: [1, 64, 0],
            ..original
        },
        ContainerOpenEvent {
            runtime_entity_id: 17,
            ..original
        },
        ContainerOpenEvent {
            window_type: protocol::WINDOW_TYPE_WORKBENCH,
            ..original
        },
    ] {
        let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
        ledger.apply(&content(descriptor(Some(0), None)));
        let generation = ledger.storage_generation();
        ledger.apply(&InventoryEvent::Open(replacement));
        assert_ne!(ledger.storage_generation(), generation);
        assert!(ledger.storage_slot_count().is_none());
    }
    let mut ledger = open(GENERIC_STORAGE_WINDOW_TYPE);
    ledger.apply(&content(descriptor(Some(0), None)));
    let generation = ledger.storage_generation();
    ledger.apply(&InventoryEvent::Close(protocol::ContainerCloseEvent {
        container: original.container,
        window_type: original.window_type,
        server_initiated: true,
    }));
    ledger.apply(&InventoryEvent::Open(original));
    assert_ne!(ledger.storage_generation(), generation);
    assert!(ledger.storage_slot_count().is_none());
}
