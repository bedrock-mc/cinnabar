use super::*;

/// Builds an actor container command with the server's persistent actor identity.
fn actor_open(actor: i64, window_id: i32) -> InventoryEvent {
    InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(window_id),
        window_type: protocol::WINDOW_TYPE_HORSE,
        position: [0; 3],
        runtime_entity_id: actor,
    })
}

/// Closes an unanswered mount request after its transport deadline.
fn expire_mount(ledger: &mut PlayerInventoryLedger) {
    assert!(ledger.request_personal_open(900));
    assert!(ledger.mark_transport_enqueued(0));
    assert!(ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS));
    ledger.request_personal_close();
}

#[test]
fn server_actor_open_supersedes_an_admitted_personal_open_without_a_rejection_close() {
    let mut ledger = ledger_with_slot_zero();
    expire_mount(&mut ledger);
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
    ledger.apply(&actor_open(-91, 7));
    assert_eq!(ledger.window_actor(), Some(-91));
    assert!(ledger.pending_batch().unwrap().is_none());

    ledger.apply(&InventoryEvent::Open(personal_open(7)));
    assert!(ledger.window_kind().is_none());
    assert!(ledger.personal_inventory_desired_open());
    assert!(ledger.pending_batch().unwrap().is_none());
    assert!(ledger.begin_click(0).is_ok());
}

#[test]
fn actor_reply_after_a_runtime_id_change_settles_without_a_timeout_close() {
    let mut ledger = ledger_with_slot_zero();
    expire_mount(&mut ledger);
    assert!(ledger.request_personal_open(901));
    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
    ledger.apply(&actor_open(-91, 8));
    assert_eq!(ledger.window_actor(), Some(-91));
    ledger.poll_timeout(2 * INVENTORY_REQUEST_TIMEOUT_MILLIS + 1);
    assert!(ledger.pending_batch().unwrap().is_none());
}

#[test]
fn server_actor_open_is_authoritative_after_cancellation_and_without_a_request() {
    for requested in [false, true] {
        let mut ledger = ledger_with_slot_zero();
        if requested {
            assert!(ledger.request_personal_open(900));
            assert!(ledger.mark_transport_enqueued(0));
            ledger.request_personal_close();
        }
        ledger.apply(&actor_open(-92, 7));
        assert_eq!(ledger.window_actor(), Some(-92));
        assert!(ledger.pending_batch().unwrap().is_none());
        ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
        assert!(ledger.pending_batch().unwrap().is_none());
    }
}

#[test]
fn server_personal_open_is_authoritative_without_a_request() {
    let mut ledger = ledger_with_slot_zero();
    ledger.apply(&InventoryEvent::Open(personal_open(7)));
    assert!(ledger.personal_inventory_desired_open());
    assert!(ledger.pending_batch().unwrap().is_none());
    assert!(ledger.begin_click(0).is_ok());
}
