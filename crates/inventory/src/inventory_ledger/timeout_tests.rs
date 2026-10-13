use super::*;

/// Expires an admitted personal open or close with no reply from the server.
fn expired_personal_ledger(closing: bool) -> PlayerInventoryLedger {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    if closing {
        ledger.apply(&InventoryEvent::Open(personal_open(2)));
        ledger.request_personal_close();
        assert!(ledger.mark_transport_enqueued(20));
    }
    assert!(ledger.poll_timeout(20 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    assert!(!ledger.personal_inventory_desired_open());
    ledger
}

/// Opens a storage screen and supplies the full contents required for chest gestures.
fn open_storage(ledger: &mut PlayerInventoryLedger, window_id: i32, window_type: i8) {
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(window_id),
        window_type,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    if window_type == GENERIC_STORAGE_WINDOW_TYPE {
        let mut slots = vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT];
        slots[0] = stack(23, 3);
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: Some(window_id),
                slot_type: Some(GENERIC_STORAGE_SLOT_TYPE),
                dynamic_id: None,
            },
            slots: Arc::from(slots),
            storage_item: NetworkItemStack::empty(),
        }));
    }
}

#[test]
fn personal_timeouts_leave_chests_and_crafting_tables_available() {
    for closing in [false, true] {
        for window_type in [GENERIC_STORAGE_WINDOW_TYPE, WORKBENCH_WINDOW_TYPE] {
            let mut ledger = expired_personal_ledger(closing);
            open_storage(&mut ledger, 3, window_type);
            assert_eq!(
                ledger.window_kind(),
                protocol::WindowKind::from_window_type(window_type)
            );
            assert!(ledger.pending_control_packet().unwrap().is_none());
            if window_type == GENERIC_STORAGE_WINDOW_TYPE {
                assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));
                assert!(ledger.begin_storage_click(0).is_ok());
            } else {
                assert_eq!(ledger.crafting_grid(), CraftingGrid::Workbench);
                assert!(ledger.begin_click(0).is_ok());
            }
        }
    }
}

#[test]
fn expired_personal_cleanup_preserves_storage_and_its_inputs() {
    let mut ledger = expired_personal_ledger(false);
    assert!(ledger.request_personal_open(42));
    open_storage(&mut ledger, 3, WORKBENCH_WINDOW_TYPE);
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(ledger.mark_transport_enqueued(30 + INVENTORY_REQUEST_TIMEOUT_MILLIS));

    let cursor = stack(31, 4);
    let ingredient = stack(32, 2);
    let craft_slot = *protocol::CRAFTING_INPUT_SLOTS.end();
    for (container, slot, stack) in [
        (CONTAINER_NAME_CURSOR, 0, cursor.clone()),
        (
            protocol::CONTAINER_NAME_CRAFT_INPUT,
            craft_slot,
            ingredient.clone(),
        ),
    ] {
        ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
            identity: protocol::SlotIdentity {
                container: ContainerIdentity {
                    window_id: None,
                    slot_type: Some(container),
                    dynamic_id: None,
                },
                slot: u16::from(slot),
            },
            stack,
            storage_item: None,
        }));
    }
    assert_eq!(ledger.cursor_stack(), Some(&cursor));
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(craft_slot)),
        Some(&ingredient)
    );
    // A close reply's payload is not correlation, even when it names the current storage.
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(3),
        window_type: WORKBENCH_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert_eq!(ledger.crafting_grid(), CraftingGrid::Workbench);
    assert_eq!(ledger.cursor_stack(), Some(&cursor));
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(craft_slot)),
        Some(&ingredient)
    );
    assert!(!ledger.resync_required());
    assert!(ledger.pending_control_packet().unwrap().is_none());
    assert!(
        ledger.personal_inventory_desired_open(),
        "the retry stays held behind storage"
    );
}

#[test]
fn timeout_requires_refresh_for_inputs_before_releasing_personal_ownership() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    let slot = *protocol::CRAFTING_INPUT_SLOTS.start();
    let ingredient = stack(32, 2);
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(protocol::CONTAINER_NAME_CRAFT_INPUT),
                dynamic_id: None,
            },
            slot: u16::from(slot),
        },
        stack: ingredient.clone(),
        storage_item: None,
    }));
    assert!(!ledger.resync_required());
    assert!(ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    assert!(ledger.resync_required());
    assert_eq!(
        ledger.target_stack(InventoryTarget::Craft(slot)),
        Some(&ingredient)
    );
}

#[test]
fn storage_reusing_an_expired_personal_id_cancels_unsent_cleanup() {
    for late_open_first in [false, true] {
        let mut ledger = expired_personal_ledger(false);
        if late_open_first {
            ledger.apply(&InventoryEvent::Open(personal_open(2)));
        }
        open_storage(&mut ledger, 2, GENERIC_STORAGE_WINDOW_TYPE);
        if !late_open_first {
            ledger.apply(&InventoryEvent::Open(personal_open(2)));
        }
        assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));
        assert!(ledger.pending_control_packet().unwrap().is_none());
        assert!(ledger.begin_storage_click(0).is_ok());
    }
}

#[test]
fn admitted_expired_cleanup_cannot_close_storage_reusing_its_id() {
    let mut ledger = expired_personal_ledger(false);
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(ledger.mark_transport_enqueued(30 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    open_storage(&mut ledger, 2, GENERIC_STORAGE_WINDOW_TYPE);
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));
    assert!(ledger.pending_control_packet().unwrap().is_none());
    assert!(ledger.begin_storage_click(0).is_ok());
}

#[test]
fn teardown_discards_expired_personal_controls_and_held_retries() {
    for teardown in ["session", "transport", "authority"] {
        let mut ledger = expired_personal_ledger(false);
        assert!(ledger.request_personal_open(42));
        match teardown {
            "session" => {
                ledger.begin_session(2);
                ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
            }
            "transport" => ledger.transport_closed(),
            "authority" => ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client)),
            _ => unreachable!(),
        }
        assert!(!ledger.personal_inventory_desired_open());
        assert!(ledger.pending_control_packet().unwrap().is_none());
        assert!(ledger.request_personal_open(42));
        assert!(ledger.pending_control_packet().unwrap().is_some());
        assert!(ledger.mark_transport_enqueued(40 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    }
}

/// Supplies the authoritative result of taking the stack installed by `open_storage`.
fn accept_storage_pickup(ledger: &mut PlayerInventoryLedger, request_id: i32) {
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id,
            containers: Arc::from([
                correction(GENERIC_STORAGE_SLOT_TYPE, 0, 0, 0),
                correction(CONTAINER_NAME_CURSOR, 0, 3, 23),
            ]),
        }]),
    }));
}

/// Creates a close reply with the supplied window identity.
fn close_reply(window_id: i32, window_type: i8) -> InventoryEvent {
    InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(window_id),
        window_type,
        server_initiated: false,
    })
}

#[test]
fn expired_personal_close_allows_storage_acknowledgements_to_preserve_pending_moves() {
    for late_open_cleanup in [false, true] {
        for storage_window_id in [2, 3] {
            for personal_ack_first in [false, true] {
                let mut ledger = expired_personal_ledger(!late_open_cleanup);
                let now = 40 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
                if late_open_cleanup {
                    ledger.apply(&InventoryEvent::Open(personal_open(2)));
                    assert!(ledger.mark_transport_enqueued(now));
                }
                open_storage(&mut ledger, storage_window_id, GENERIC_STORAGE_WINDOW_TYPE);
                let request = ledger.begin_storage_click(0).unwrap();
                assert!(ledger.mark_transport_enqueued(now + 1));
                ledger.request_storage_close();
                assert!(ledger.mark_transport_enqueued(now + 2));
                let personal_ack = close_reply(2, PERSONAL_INVENTORY_WINDOW_TYPE);
                if personal_ack_first {
                    ledger.apply(&personal_ack);
                }
                ledger.apply(&close_reply(storage_window_id, GENERIC_STORAGE_WINDOW_TYPE));
                assert_eq!(ledger.window_kind(), None);
                assert_eq!(ledger.pending_request_id(), Some(request));

                // Reuse the chest ID while its old move still awaits the server's answer.
                open_storage(&mut ledger, storage_window_id, GENERIC_STORAGE_WINDOW_TYPE);
                if !personal_ack_first {
                    ledger.apply(&personal_ack);
                }
                accept_storage_pickup(&mut ledger, request);
                assert_eq!(ledger.cursor_stack(), Some(&stack(23, 3)));
                assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));
                assert_eq!(ledger.pending_request_id(), None);
                assert!(!ledger.resync_required());
            }
        }
    }
}

#[test]
fn expired_personal_reply_cannot_acknowledge_an_unsent_storage_close() {
    let mut ledger = expired_personal_ledger(true);
    let now = 40 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
    open_storage(&mut ledger, 3, GENERIC_STORAGE_WINDOW_TYPE);
    let request = ledger.begin_storage_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(now));
    ledger.request_storage_close();
    let reply = close_reply(3, GENERIC_STORAGE_WINDOW_TYPE);
    ledger.apply(&reply);
    assert!(ledger.window_kind().is_some());
    assert!(ledger.pending_control_packet().unwrap().is_some());
    assert_eq!(ledger.pending_request_id(), Some(request));

    assert!(ledger.mark_transport_enqueued(now + 1));
    ledger.apply(&reply);
    assert_eq!(ledger.window_kind(), None);
    open_storage(&mut ledger, 4, GENERIC_STORAGE_WINDOW_TYPE);
    accept_storage_pickup(&mut ledger, request);
    assert_eq!(ledger.cursor_stack(), Some(&stack(23, 3)));
    assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));
    assert!(!ledger.resync_required());
}

#[test]
fn storage_close_before_late_personal_open_preserves_moves_through_retry() {
    let mut ledger = expired_personal_ledger(false);
    let now = 40 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
    assert!(ledger.request_personal_open(42));
    open_storage(&mut ledger, 3, GENERIC_STORAGE_WINDOW_TYPE);
    let request = ledger.begin_storage_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(now));
    ledger.request_storage_close();
    assert!(ledger.mark_transport_enqueued(now + 1));
    ledger.apply(&close_reply(3, GENERIC_STORAGE_WINDOW_TYPE));
    assert_eq!(ledger.window_kind(), None);
    assert!(ledger.pending_control_packet().unwrap().is_none());

    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(ledger.mark_transport_enqueued(now + 2));
    ledger.apply(&close_reply(2, PERSONAL_INVENTORY_WINDOW_TYPE));
    assert!(ledger.pending_control_packet().unwrap().is_some());
    assert!(ledger.mark_transport_enqueued(now + 3));
    ledger.apply(&InventoryEvent::Open(personal_open(4)));
    accept_storage_pickup(&mut ledger, request);
    assert_eq!(ledger.cursor_stack(), Some(&stack(23, 3)));
    assert_eq!(ledger.pending_request_id(), None);
    assert!(ledger.personal_inventory_desired_open());
    assert!(!ledger.resync_required());
}
