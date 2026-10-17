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
fn personal_open_during_pending_storage_open_can_reopen_after_storage_close() {
    let mut ledger = ledger_with_slot_zero();

    // The chest interaction has left, but its Open reply has not arrived yet.
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    open_chest(&mut ledger, 3);
    ledger.request_storage_close();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(3),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        server_initiated: false,
    }));

    assert!(ledger.request_personal_open(42));
    assert!(ledger.pending_control_packet().unwrap().is_none());
    assert!(!ledger.mark_transport_enqueued(30));
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(ledger.personal_inventory_desired_open());
    assert_eq!(ledger.begin_click(0).unwrap(), -3);
}

#[test]
fn storage_open_supersedes_only_an_unacknowledged_personal_open() {
    for admitted in [false, true] {
        for cancelled in [false, true] {
            let mut ledger = ledger_with_slot_zero();
            assert!(ledger.request_personal_open(42));
            if admitted {
                assert!(ledger.mark_transport_enqueued(10));
            }
            if cancelled {
                ledger.request_personal_close();
            }
            open_chest(&mut ledger, 3);
            assert_eq!(
                ledger.window_kind(),
                protocol::WindowKind::from_window_type(GENERIC_STORAGE_WINDOW_TYPE)
            );
            assert!(!ledger.personal_inventory_desired_open());
            assert!(ledger.pending_batch().unwrap().is_none());
            assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));

            // A later personal reply must be cleaned up without taking the chest's cells.
            if admitted {
                ledger.apply(&InventoryEvent::Open(personal_open(2)));
                assert!(ledger.mark_transport_enqueued(20));
                ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
                    container: ContainerIdentity::window(2),
                    window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
                    server_initiated: false,
                }));
                assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));
                assert!(!ledger.personal_inventory_desired_open());
            }
            assert!(ledger.begin_storage_click(0).is_ok());
        }
    }
}

#[test]
fn late_personal_cleanup_cannot_close_storage_reusing_its_id() {
    for cancelled in [false, true] {
        for cleanup_queued_first in [false, true] {
            let mut ledger = ledger_with_slot_zero();
            assert!(ledger.request_personal_open(42));
            assert!(ledger.mark_transport_enqueued(10));
            if cancelled {
                ledger.request_personal_close();
            }
            open_chest(&mut ledger, 3);

            let personal_window_id = if cleanup_queued_first { 2 } else { 3 };
            ledger.apply(&InventoryEvent::Open(personal_open(personal_window_id)));
            if cleanup_queued_first {
                assert_eq!(
                    protocol::test_support::packets::container_close(
                        &ledger.pending_control_packet().unwrap().unwrap()
                    ),
                    Some(protocol::test_support::packets::ContainerCloseObservation {
                        container_id: u8::try_from(personal_window_id).unwrap(),
                        server_initiated: false,
                    })
                );
                open_chest(&mut ledger, personal_window_id);
            }

            assert!(ledger.pending_batch().unwrap().is_none());
            assert!(!ledger.mark_transport_enqueued(20));
            assert_eq!(
                ledger.window_kind(),
                protocol::WindowKind::from_window_type(GENERIC_STORAGE_WINDOW_TYPE)
            );
            assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));
            assert!(!ledger.personal_inventory_desired_open());
            assert!(ledger.begin_storage_click(0).is_ok());
        }
    }
}

#[test]
fn storage_open_does_not_displace_acknowledged_personal_inventory() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);

    open_chest(&mut ledger, 3);
    assert_eq!(ledger.window_kind(), None);
    assert!(ledger.personal_inventory_desired_open());
    assert_eq!(
        protocol::test_support::packets::container_close(
            &ledger.pending_control_packet().unwrap().unwrap()
        ),
        Some(protocol::test_support::packets::ContainerCloseObservation {
            container_id: 3,
            server_initiated: false,
        })
    );
    assert_eq!(ledger.begin_click(0).unwrap(), -3);
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
fn superseded_open_reconciles_only_the_latest_intent() {
    for reopen in [false, true] {
        let mut ledger = ledger_with_slot_zero();
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        open_chest(&mut ledger, 3);
        ledger.request_storage_close();
        assert!(ledger.mark_transport_enqueued(20));
        ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(3),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            server_initiated: false,
        }));
        assert!(ledger.request_personal_open(42));
        ledger.request_personal_close();
        if reopen {
            assert!(ledger.request_personal_open(42));
        }
        assert!(!ledger.mark_transport_enqueued(30));
        ledger.apply(&InventoryEvent::Open(personal_open(0)));
        assert_eq!(ledger.personal_inventory_desired_open(), reopen);
        assert_eq!(ledger.pending_control_packet().unwrap().is_some(), !reopen);
        if !reopen {
            assert!(ledger.mark_transport_enqueued(40));
            ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
                container: ContainerIdentity::window(0),
                window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
                server_initiated: false,
            }));
            assert!(ledger.request_personal_open(42));
            assert!(ledger.mark_transport_enqueued(50));
            ledger.apply(&InventoryEvent::Open(personal_open(0)));
        }
        assert!(matches!(ledger.personal, Some(PersonalWindow::Open { .. })));
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
    assert_eq!(ledger.pending_request_id(), Some(request));
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
fn rejected_storage_opens_keep_only_the_latest_close_desire() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    ledger.request_personal_close();
    for window_id in 3..=8 {
        open_chest(&mut ledger, window_id);
    }
    assert_eq!(
        protocol::test_support::packets::container_close(
            &ledger.pending_control_packet().unwrap().unwrap()
        )
        .unwrap()
        .container_id,
        8
    );
    assert!(ledger.mark_transport_enqueued(20));
    assert_eq!(
        protocol::test_support::packets::container_close(
            &ledger.pending_control_packet().unwrap().unwrap()
        )
        .unwrap()
        .container_id,
        2
    );
    assert!(ledger.mark_transport_enqueued(30));
    assert!(!ledger.mark_transport_enqueued(40));
    for window_id in [2, 8] {
        ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(window_id),
            window_type: NO_CONTAINER_WINDOW_TYPE,
            server_initiated: false,
        }));
    }
    assert!(ledger.controls.personal.confirmed.is_none());
    assert!(ledger.controls.storage.confirmed.is_none());
    assert!(ledger.request_personal_open(42));
    assert!(ledger.pending_control_packet().unwrap().is_some());
}
