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
