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

    ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.request_personal_open(42));
    assert!(
        ledger.pending_batch().unwrap().is_some(),
        "the chest lifecycle must leave the next personal Open available"
    );
    assert!(ledger.mark_transport_enqueued(20 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(matches!(
        ledger.personal,
        Some(PersonalWindow::Opening { .. })
    ));
    ledger.apply(&InventoryEvent::Open(personal_open(4)));
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
            assert!(!ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
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
fn superseded_open_reply_cannot_acknowledge_a_new_personal_generation() {
    for timed_out in [false, true] {
        for new_window_id in [2, 4] {
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
            let now = if timed_out {
                10 + INVENTORY_REQUEST_TIMEOUT_MILLIS
            } else {
                30
            };
            ledger.poll_timeout(now);
            assert!(ledger.request_personal_open(42));
            assert!(ledger.mark_transport_enqueued(now));
            let generation = ledger.personal.unwrap().generation();

            ledger.apply(&InventoryEvent::Open(personal_open(2)));
            assert!(
                matches!(ledger.personal, Some(PersonalWindow::Opening { generation: active, .. }) if active == generation)
            );
            assert!(ledger.pending_batch().unwrap().is_none());

            ledger.apply(&InventoryEvent::Open(personal_open(new_window_id)));
            assert!(
                matches!(ledger.personal, Some(PersonalWindow::Open { generation: active, window_id, .. }) if active == generation && window_id == new_window_id)
            );
            assert!(ledger.pending_batch().unwrap().is_none());
            ledger.request_personal_close();
            assert_eq!(
                protocol::test_support::packets::container_close(
                    &ledger.pending_control_packet().unwrap().unwrap()
                )
                .unwrap()
                .container_id,
                u8::try_from(new_window_id).unwrap()
            );
        }
    }
}

#[test]
fn superseded_personal_cleanup_cannot_acknowledge_a_storage_close() {
    for storage_close_sent in [false, true] {
        let mut ledger = ledger_with_slot_zero();
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        open_chest(&mut ledger, 3);
        let generation = ledger.storage_generation();
        ledger.apply(&InventoryEvent::Open(personal_open(2)));
        assert!(ledger.mark_transport_enqueued(20));
        let request = ledger.begin_storage_click(0).unwrap();
        assert!(ledger.mark_transport_enqueued(30));
        ledger.request_storage_close();
        if storage_close_sent {
            assert!(ledger.mark_transport_enqueued(40));
        }
        let reply = InventoryEvent::Close(ContainerCloseEvent {
            container: ContainerIdentity::window(3),
            window_type: GENERIC_STORAGE_WINDOW_TYPE,
            server_initiated: false,
        });
        ledger.apply(&reply);
        assert_eq!(ledger.storage_generation(), generation);
        assert_eq!(ledger.pending_request_id(), Some(request));
        assert_eq!(
            ledger.pending_control_packet().unwrap().is_some(),
            !storage_close_sent
        );

        if !storage_close_sent {
            assert!(ledger.mark_transport_enqueued(50));
        }
        ledger.apply(&reply);
        assert_eq!(ledger.storage_generation(), None);
        assert_eq!(ledger.pending_request_id(), Some(request));
        assert!(!ledger.resync_required());
    }
}

#[test]
fn old_storage_close_reply_cannot_close_a_reused_window_id() {
    let mut ledger = ledger_with_slot_zero();
    open_chest(&mut ledger, 3);
    ledger.request_storage_close();
    assert!(ledger.mark_transport_enqueued(10));
    open_chest(&mut ledger, 3);
    let generation = ledger.storage_generation();
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(3),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert_eq!(ledger.storage_generation(), generation);
    assert_eq!(ledger.storage_stack(0), Some(&stack(23, 3)));
    assert!(ledger.begin_storage_click(0).is_ok());
}

#[test]
fn an_unsent_storage_close_ignores_an_uncorrelated_acknowledgement() {
    let mut ledger = ledger_with_slot_zero();
    open_chest(&mut ledger, 3);
    let generation = ledger.storage_generation();
    let request = ledger.begin_storage_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.request_storage_close();
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(3),
        window_type: GENERIC_STORAGE_WINDOW_TYPE,
        server_initiated: false,
    }));
    assert_eq!(ledger.storage_generation(), generation);
    assert_eq!(ledger.pending_request_id(), Some(request));
    assert!(ledger.pending_control_packet().unwrap().is_some());
}

#[test]
fn cleanup_close_reply_cannot_acknowledge_a_later_personal_close() {
    let mut ledger = ledger_with_slot_zero();
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(ledger.mark_transport_enqueued(10));
    acknowledge_personal_open(&mut ledger, 4);
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(20));
    let reply = InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(4),
        window_type: NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    });
    ledger.apply(&reply);
    assert!(matches!(
        ledger.personal,
        Some(PersonalWindow::Closing { .. })
    ));
    ledger.apply(&reply);
    assert!(ledger.personal.is_none());
}

#[test]
fn authority_change_keeps_sent_personal_open_correlation_until_its_reply() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(20));
    let generation = ledger.personal.unwrap().generation();

    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(
        matches!(ledger.personal, Some(PersonalWindow::Opening { generation: active, .. }) if active == generation)
    );
    ledger.apply(&InventoryEvent::Open(personal_open(4)));
    assert!(
        matches!(ledger.personal, Some(PersonalWindow::Open { generation: active, window_id: 4, .. }) if active == generation)
    );
    assert!(ledger.pending_control_packet().unwrap().is_none());
}
