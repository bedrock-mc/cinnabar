use super::*;

/// Admits one inventory request addressed to a mount rather than the player.
fn opening_mount() -> PlayerInventoryLedger {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_actor_open(900, -91));
    assert!(ledger.mark_transport_enqueued(0));
    ledger
}

/// Builds a horse-window reply using its actor's persistent identity.
fn actor_open(actor_unique_id: i64) -> InventoryEvent {
    InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(7),
        window_type: protocol::WINDOW_TYPE_HORSE,
        position: [0; 3],
        runtime_entity_id: actor_unique_id,
    })
}

/// Completes an admitted close without issuing an independent server command.
fn acknowledge_close(ledger: &mut PlayerInventoryLedger, window_id: i32, window_type: i8) {
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(window_id),
        window_type,
        server_initiated: false,
    }));
}

#[test]
fn mounted_reply_finishes_the_request_without_a_personal_reset_close() {
    let mut ledger = opening_mount();
    ledger.apply(&actor_open(-91));
    assert_eq!(ledger.window_actor(), Some(-91));
    ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.pending_batch().unwrap().is_none());
    ledger.request_storage_close();
    assert_eq!(
        ledger.pending_batch().unwrap().unwrap().0,
        protocol::container_close_packet(7).unwrap()
    );
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(7),
        window_type: protocol::WINDOW_TYPE_HORSE,
        server_initiated: false,
    }));
    assert!(ledger.request_actor_open(900, -91));
    assert_eq!(
        ledger.pending_batch().unwrap().unwrap().0,
        protocol::open_inventory_packet(900).unwrap()
    );
}

#[test]
fn mounted_reply_uses_the_latest_desire_when_opening_was_cancelled() {
    for reopen in [false, true] {
        let mut ledger = opening_mount();
        ledger.request_personal_close();
        if reopen {
            assert!(ledger.request_actor_open(900, -91));
        }
        ledger.apply(&actor_open(-91));
        assert_eq!(ledger.storage_generation().is_some(), reopen);
        if reopen {
            assert!(ledger.pending_batch().unwrap().is_none());
        } else {
            assert_eq!(
                ledger.pending_batch().unwrap().unwrap().0,
                protocol::container_close_packet(7).unwrap()
            );
        }
    }
}

#[test]
fn cancelled_mounted_reply_after_timeout_stays_closed() {
    for reset_sent in [false, true] {
        let mut ledger = opening_mount();
        ledger.request_personal_close();
        assert!(ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS));
        if reset_sent {
            assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS));
        }

        ledger.apply(&actor_open(-91));
        assert!(ledger.storage_generation().is_none());
        assert!(!ledger.personal_inventory_desired_open());
        assert_eq!(
            ledger.pending_batch().unwrap().unwrap().0,
            protocol::container_close_packet(7).unwrap()
        );
        assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
        acknowledge_close(&mut ledger, 7, protocol::WINDOW_TYPE_HORSE);
        if reset_sent {
            acknowledge_close(
                &mut ledger,
                protocol::PLAYER_INVENTORY_WINDOW_ID,
                PERSONAL_INVENTORY_WINDOW_TYPE,
            );
        }
        assert!(ledger.pending_batch().unwrap().is_none());
    }
}

#[test]
fn old_mounted_reply_preserves_a_newer_inventory_target() {
    for timed_out in [false, true] {
        for newer_actor in [None, Some(-92)] {
            let mut ledger = opening_mount();
            ledger.request_personal_close();
            if timed_out {
                assert!(ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS));
            }
            if let Some(actor) = newer_actor {
                assert!(ledger.request_actor_open(901, actor));
            } else {
                assert!(ledger.request_personal_open(42));
            }

            ledger.apply(&actor_open(-91));
            assert!(ledger.personal_inventory_desired_open());
            assert!(ledger.storage_generation().is_none());
            assert_eq!(
                ledger.pending_batch().unwrap().unwrap().0,
                protocol::container_close_packet(7).unwrap()
            );
            assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
            acknowledge_close(&mut ledger, 7, protocol::WINDOW_TYPE_HORSE);
            let runtime_id = if newer_actor.is_some() { 901 } else { 42 };
            assert_eq!(
                ledger.pending_batch().unwrap().unwrap().0,
                protocol::open_inventory_packet(runtime_id).unwrap()
            );
            assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 2));
            let reply = newer_actor.map_or_else(
                || InventoryEvent::Open(personal_open(protocol::PLAYER_INVENTORY_WINDOW_ID)),
                actor_open,
            );
            ledger.apply(&reply);
            assert_eq!(ledger.window_actor(), newer_actor);
            assert!(ledger.pending_batch().unwrap().is_none());
        }
    }
}

#[test]
fn late_mounted_reply_preserves_a_newer_admitted_open() {
    for newer_actor in [None, Some(-92)] {
        let mut ledger = opening_mount();
        ledger.request_personal_close();
        assert!(ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS));
        assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS));
        acknowledge_close(
            &mut ledger,
            protocol::PLAYER_INVENTORY_WINDOW_ID,
            PERSONAL_INVENTORY_WINDOW_TYPE,
        );
        if let Some(actor) = newer_actor {
            assert!(ledger.request_actor_open(901, actor));
        } else {
            assert!(ledger.request_personal_open(42));
        }
        assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));

        ledger.apply(&actor_open(-91));
        assert!(ledger.personal_inventory_desired_open());
        assert!(ledger.storage_generation().is_none());
        assert_eq!(
            ledger.pending_batch().unwrap().unwrap().0,
            protocol::container_close_packet(7).unwrap()
        );
        assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 2));
        acknowledge_close(&mut ledger, 7, protocol::WINDOW_TYPE_HORSE);
        let reply = newer_actor.map_or_else(
            || InventoryEvent::Open(personal_open(protocol::PLAYER_INVENTORY_WINDOW_ID)),
            actor_open,
        );
        ledger.apply(&reply);
        assert_eq!(ledger.window_actor(), newer_actor);
        if newer_actor.is_none() {
            assert!(ledger.personal_inventory_desired_open());
            assert!(matches!(
                ledger.personal,
                Some(crate::inventory_ledger::PersonalWindow::Open { .. })
            ));
        }
        assert!(ledger.pending_batch().unwrap().is_none());
    }
}

#[test]
fn mounted_cancellation_keeps_unrelated_server_windows_authoritative() {
    for reply in [storage_open(7), actor_open(-92)] {
        let mut ledger = opening_mount();
        ledger.request_personal_close();
        ledger.apply(&reply);
        assert!(ledger.storage_generation().is_some());
        assert!(ledger.pending_batch().unwrap().is_none());
    }
}

/// Expires the original open and completes the reset before another target is admitted.
fn reset_unanswered_open(ledger: &mut PlayerInventoryLedger, now: u64) {
    assert!(ledger.poll_timeout(now));
    assert!(ledger.mark_transport_enqueued(now));
    acknowledge_close(
        ledger,
        protocol::PLAYER_INVENTORY_WINDOW_ID,
        PERSONAL_INVENTORY_WINDOW_TYPE,
    );
}

/// Builds distinguishable replies for retries of the same actor.
fn actor_open_in_window(actor_unique_id: i64, window_id: i32) -> InventoryEvent {
    let InventoryEvent::Open(mut open) = actor_open(actor_unique_id) else {
        unreachable!()
    };
    open.container = ContainerIdentity::window(window_id);
    InventoryEvent::Open(open)
}

#[test]
fn mounted_retry_recovers_after_missing_open_replies() {
    for missing_replies in 1..=3 {
        let mut ledger = opening_mount();
        for attempt in 1..=missing_replies {
            let now = attempt * (INVENTORY_REQUEST_TIMEOUT_MILLIS + 1);
            reset_unanswered_open(&mut ledger, now);
            assert!(ledger.mark_transport_enqueued(now));
        }

        ledger.apply(&actor_open(-91));
        assert_eq!(ledger.window_actor(), Some(-91));
        assert!(ledger.pending_control_packet().unwrap().is_none());
        ledger.poll_timeout((missing_replies + 1) * (INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
        assert_eq!(ledger.window_actor(), Some(-91));
        assert!(ledger.pending_control_packet().unwrap().is_none());
    }
}

#[test]
fn mounted_retry_cancellation_rejects_a_remaining_late_reply() {
    let mut ledger = opening_mount();
    reset_unanswered_open(&mut ledger, INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
    ledger.apply(&actor_open(-91));
    assert_eq!(ledger.window_actor(), Some(-91));
    ledger.request_storage_close();
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 2));
    acknowledge_close(&mut ledger, 7, protocol::WINDOW_TYPE_HORSE);

    ledger.apply(&actor_open_in_window(-91, 8));
    assert!(ledger.storage_generation().is_none());
    assert!(!ledger.personal_inventory_desired_open());
    assert_eq!(
        ledger.pending_control_packet().unwrap().unwrap(),
        protocol::container_close_packet(8).unwrap()
    );
}

#[test]
fn cancelled_mount_retry_rejects_both_late_replies_in_either_order() {
    for retry_times_out in [false, true] {
        for reply_ids in [[7, 8], [8, 7]] {
            let mut ledger = opening_mount();
            reset_unanswered_open(&mut ledger, INVENTORY_REQUEST_TIMEOUT_MILLIS);
            assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
            ledger.request_personal_close();
            let now = if retry_times_out {
                let now = 2 * INVENTORY_REQUEST_TIMEOUT_MILLIS + 1;
                reset_unanswered_open(&mut ledger, now);
                now + 1
            } else {
                INVENTORY_REQUEST_TIMEOUT_MILLIS + 2
            };

            for window_id in reply_ids {
                ledger.apply(&actor_open_in_window(-91, window_id));
                assert!(ledger.storage_generation().is_none());
                assert!(!ledger.personal_inventory_desired_open());
                assert_eq!(
                    ledger.pending_batch().unwrap().unwrap().0,
                    protocol::container_close_packet(window_id).unwrap()
                );
                assert!(ledger.mark_transport_enqueued(now));
                acknowledge_close(&mut ledger, window_id, protocol::WINDOW_TYPE_HORSE);
            }
            assert!(ledger.pending_batch().unwrap().is_none());
        }
    }
}

#[test]
fn stale_mount_reply_preserves_newer_storage_and_its_pending_items() {
    for old_window_id in [7, 8] {
        let mut ledger = opening_mount();
        ledger.request_personal_close();
        reset_unanswered_open(&mut ledger, INVENTORY_REQUEST_TIMEOUT_MILLIS);
        assert!(ledger.request_actor_open(901, -92));
        assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
        ledger.apply(&actor_open_in_window(-92, 8));
        let item = protocol::NetworkItemStack {
            network_id: 6,
            stack_network_id: 13,
            count: 3,
            ..Default::default()
        };
        let slot_count = *protocol::WindowKind::Horse
            .content_lengths()
            .iter()
            .max()
            .unwrap();
        let mut slots = vec![protocol::NetworkItemStack::empty(); slot_count];
        slots[2] = item;
        ledger.apply(&InventoryEvent::Content(protocol::InventoryContentEvent {
            container: ContainerIdentity::window(8),
            slots: std::sync::Arc::from(slots),
            storage_item: protocol::NetworkItemStack::empty(),
        }));
        let request = ledger.begin_storage_click(2).unwrap();
        assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 2));
        let generation = ledger.storage_generation();
        let cursor = ledger.cursor_stack().cloned();
        let contents = ledger.confirmed.clone();

        ledger.apply(&actor_open_in_window(-91, old_window_id));
        assert_eq!(ledger.window_actor(), Some(-92));
        assert_eq!(ledger.storage_generation(), generation);
        assert_eq!(ledger.pending_request_id(), Some(request));
        assert_eq!(ledger.cursor_stack(), cursor.as_ref());
        assert_eq!(ledger.confirmed.storage_len(), contents.storage_len());
        for slot in 0..contents.storage_len() {
            let cell = crate::inventory_ledger::Cell::Storage(slot as u8);
            assert_eq!(ledger.confirmed.get(cell), contents.get(cell));
        }
        assert_eq!(
            ledger.pending_control_packet().unwrap(),
            (old_window_id != 8).then(|| protocol::container_close_packet(old_window_id).unwrap())
        );
        if old_window_id != 8 {
            assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 3));
            acknowledge_close(&mut ledger, old_window_id, protocol::WINDOW_TYPE_HORSE);
            assert_eq!(ledger.storage_generation(), generation);
            assert_eq!(ledger.pending_request_id(), Some(request));
        }
    }
}

#[test]
fn personal_target_change_closes_the_admitted_window_before_opening_mount() {
    for reply_first in [false, true] {
        let mut ledger = ledger_with_slot_zero();
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(0));
        if reply_first {
            ledger.apply(&InventoryEvent::Open(personal_open(2)));
        }
        ledger.request_personal_close();
        assert!(ledger.request_actor_open(900, -91));
        if !reply_first {
            assert!(ledger.pending_batch().unwrap().is_none());
            ledger.apply(&InventoryEvent::Open(personal_open(2)));
        }
        assert!(ledger.personal_inventory_desired_open());
        assert_eq!(
            ledger.pending_batch().unwrap().unwrap().0,
            protocol::container_close_packet(2).unwrap()
        );
        assert!(ledger.mark_transport_enqueued(1));
        acknowledge_close(&mut ledger, 2, PERSONAL_INVENTORY_WINDOW_TYPE);
        assert_eq!(
            ledger.pending_batch().unwrap().unwrap().0,
            protocol::open_inventory_packet(900).unwrap()
        );
        assert!(ledger.mark_transport_enqueued(2));
        ledger.apply(&actor_open(-91));
        assert_eq!(ledger.window_actor(), Some(-91));
        assert!(ledger.pending_batch().unwrap().is_none());
    }
}

#[test]
fn personal_retry_keeps_cancellation_after_the_older_reply_confirms_it() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    reset_unanswered_open(&mut ledger, INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    assert!(ledger.personal_generation_for_gesture().is_some());
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 2));
    acknowledge_close(&mut ledger, 2, PERSONAL_INVENTORY_WINDOW_TYPE);

    ledger.apply(&InventoryEvent::Open(personal_open(3)));
    assert!(!ledger.personal_inventory_desired_open());
    assert!(ledger.personal.is_none());
    assert_eq!(
        ledger.pending_control_packet().unwrap().unwrap(),
        protocol::container_close_packet(3).unwrap()
    );
}

#[test]
fn stale_mount_close_does_not_cover_a_newer_personal_reply_with_the_same_id() {
    let mut ledger = opening_mount();
    ledger.request_personal_close();
    reset_unanswered_open(&mut ledger, INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
    ledger.apply(&actor_open(-91));
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 2));

    ledger.apply(&InventoryEvent::Open(personal_open(7)));
    assert!(ledger.personal_inventory_desired_open());
    acknowledge_close(&mut ledger, 7, protocol::WINDOW_TYPE_HORSE);
    assert!(ledger.personal_inventory_desired_open());
    assert!(ledger.begin_click(0).is_ok());
}

#[test]
fn stale_personal_close_timeout_preserves_the_newer_personal_screen() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    reset_unanswered_open(&mut ledger, INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.request_personal_open(43));
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
    ledger.apply(&InventoryEvent::Open(personal_open(2)));
    ledger.apply(&InventoryEvent::Open(personal_open(3)));
    assert_eq!(
        ledger.pending_control_packet().unwrap().unwrap(),
        protocol::container_close_packet(2).unwrap()
    );
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 2));
    assert!(!ledger.poll_timeout(2 * INVENTORY_REQUEST_TIMEOUT_MILLIS + 2));
    assert!(ledger.personal_inventory_desired_open());
    assert!(ledger.begin_click(0).is_ok());
}
