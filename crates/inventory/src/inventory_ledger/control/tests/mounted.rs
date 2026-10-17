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
fn mounted_cancellation_keeps_unrelated_server_windows_authoritative() {
    for reply in [storage_open(7), actor_open(-92)] {
        let mut ledger = opening_mount();
        ledger.request_personal_close();
        ledger.apply(&reply);
        assert!(ledger.storage_generation().is_some());
        assert!(ledger.pending_batch().unwrap().is_none());
    }
}
