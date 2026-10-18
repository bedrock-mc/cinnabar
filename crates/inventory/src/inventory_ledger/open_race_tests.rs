use super::*;

/// Applies the delayed chest Open and its authoritative contents.
fn open_chest(ledger: &mut PlayerInventoryLedger, window_id: i32) {
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(window_id),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        position: [1, 64, 1],
        runtime_entity_id: -1,
    }));
    let mut slots = vec![NetworkItemStack::empty(); SMALL_STORAGE_SLOT_COUNT];
    slots[0] = stack(23, 3);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(window_id),
            slot_type: Some(GENERIC_STORAGE_SLOT_TYPE),
            dynamic_id: None,
        },
        slots: slots.into(),
        storage_item: NetworkItemStack::empty(),
    }));
}

#[test]
fn superseded_personal_open_rolls_back_unsent_moves_and_recovers_admitted_moves() {
    for admitted in [false, true] {
        let mut ledger = ledger_with_slot_zero();
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        ledger.begin_click(0).unwrap();
        if admitted {
            assert!(ledger.mark_transport_enqueued(20));
        }

        open_chest(&mut ledger, 3);
        assert_eq!(ledger.pending_request_id(), None);
        assert_eq!(
            ledger.target_stack(InventoryTarget::Player(0)),
            Some(&stack(9, 32))
        );
        assert!(ledger.cursor_stack().is_none());
        assert_eq!(ledger.resync_required(), admitted);
        assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));
    }
}

#[test]
fn close_then_open_before_admission_needs_no_wire_transition() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    ledger.begin_world_drop(0, Some(1)).unwrap();
    ledger.request_personal_close();
    assert!(ledger.request_personal_open(42));
    assert!(matches!(ledger.personal, Some(PersonalWindow::Open { .. })));
    assert_eq!(ledger.controls.personal.confirmed.unwrap().window_id, 2);
    assert!(ledger.pending_control_packet().unwrap().is_none());
    assert!(ledger.mark_transport_enqueued(20));
    assert!(!ledger.mark_transport_enqueued(30));
}

#[test]
fn current_storage_close_waits_for_the_previous_storage_reply() {
    let mut ledger = ledger_with_slot_zero();
    open_chest(&mut ledger, 3);
    ledger.request_storage_close();
    assert!(ledger.mark_transport_enqueued(10));
    open_chest(&mut ledger, 4);
    let generation = ledger.storage_generation();
    let request = ledger.begin_storage_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.request_storage_close();
    // Cursor cleanup can leave while the earlier close awaits its acknowledgement.
    assert!(ledger.mark_transport_enqueued(29));
    assert!(!ledger.mark_transport_enqueued(30));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(3),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert_eq!(ledger.storage_generation(), generation);
    assert_eq!(ledger.pending_request_id(), Some(request));
    assert!(ledger.mark_transport_enqueued(40));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(4),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert_eq!(ledger.storage_generation(), None);
    assert_eq!(ledger.pending_request_id(), None);
}

#[test]
fn server_close_does_not_release_the_client_request_slot() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: true,
    }));
    assert!(ledger.personal.is_none());
    assert!(ledger.request_personal_open(42));
    assert!(!ledger.mark_transport_enqueued(30));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert!(ledger.mark_transport_enqueued(40));
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(ledger.personal_inventory_desired_open());
}

#[test]
fn authority_changes_keep_the_outstanding_open_until_reconciliation() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    assert!(ledger.request_personal_open(42));
    assert!(!ledger.mark_transport_enqueued(20));
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(matches!(ledger.personal, Some(PersonalWindow::Open { .. })));
    assert_eq!(ledger.controls.personal.confirmed.unwrap().window_id, 2);
    assert!(ledger.pending_control_packet().unwrap().is_none());
}

#[test]
fn superseded_open_timeout_does_not_erase_storage_predictions() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    open_chest(&mut ledger, 3);
    let request = ledger.begin_storage_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    let cursor = ledger.cursor_stack().cloned();
    assert!(cursor.is_some());
    assert!(!ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    assert_eq!(ledger.cursor_stack(), cursor.as_ref());
    assert_eq!(ledger.pending_request_id(), Some(request));
    assert_eq!(ledger.storage.as_ref().unwrap().window_id, 3);
}

#[test]
fn server_storage_open_supersedes_personal_inventory_at_each_request_stage() {
    for stage in 0..2 {
        for cancelled in [false, true] {
            let mut ledger = ledger_with_slot_zero();
            assert!(ledger.request_personal_open(42));
            if stage > 0 {
                assert!(ledger.mark_transport_enqueued(0));
            }
            if cancelled {
                ledger.request_personal_close();
            }
            open_chest(&mut ledger, 3);
            assert!(!ledger.personal_inventory_desired_open());
            assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));
            assert!(ledger.pending_batch().unwrap().is_none());
            assert!(ledger.begin_storage_click(0).is_ok());
        }
    }
}

#[test]
fn a_personal_open_replaces_storage_while_storage_keeps_the_active_personal_guard() {
    let mut ledger = ledger_with_slot_zero();
    open_chest(&mut ledger, 3);
    ledger.apply(&InventoryEvent::Open(personal_open(3)));
    assert!(ledger.storage_generation().is_none());
    assert!(ledger.personal_inventory_desired_open());
    assert!(ledger.pending_batch().unwrap().is_none());
    assert!(ledger.begin_click(0).is_ok());

    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    open_chest(&mut ledger, 3);
    assert!(ledger.window_kind().is_none());
    assert!(ledger.personal_inventory_desired_open());
    assert!(ledger.pending_batch().unwrap().is_none());
    assert!(ledger.begin_click(0).is_ok());
}
