use super::*;

#[test]
fn missing_personal_open_ack_keeps_the_cursor_return_owned() {
    for (reopen, early_reopen, late_ack) in [
        (false, false, false),
        (true, false, false),
        (true, true, false),
        (false, false, true),
        (true, false, true),
        (true, true, true),
    ] {
        let mut ledger = ledger_with_slot_zero();
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        let take = ledger.begin_click(0).unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        accept_cursor_move(&mut ledger, take, false);
        ledger.request_personal_close();
        let returning = ledger.newest_request().expect("cursor return").request_id;
        assert!(ledger.mark_transport_enqueued(30));
        if early_reopen {
            assert!(ledger.request_personal_open(42));
        }
        ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
        if reopen && !early_reopen {
            assert!(ledger.request_personal_open(42));
        }
        if late_ack {
            ledger.apply(&InventoryEvent::Open(personal_open(7)));
        }
        accept_cursor_move(&mut ledger, returning, true);
        assert_eq!(ledger.displayed_stack(0).map(|stack| stack.count), Some(32));
        assert!(ledger.cursor_stack().is_none());
        assert!(!ledger.resync_required());
        assert_eq!(ledger.personal_inventory_desired_open(), reopen || late_ack);
    }
}

#[test]
fn canceling_an_unanswered_close_keeps_admitted_gestures() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    ledger.request_personal_close();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.begin_click(0).is_ok());
}

#[test]
fn overdue_cursor_return_does_not_block_a_recovery_open() {
    let mut ledger = ledger_with_slot_zero();
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    let take = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    accept_cursor_move(&mut ledger, take, false);
    ledger.request_personal_close();
    let returning = ledger.newest_request().unwrap().request_id;
    assert!(ledger.mark_transport_enqueued(30));
    ledger.poll_timeout(30 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.request_personal_open(42));
    assert_eq!(
        ledger.pending_batch().unwrap().unwrap().0,
        protocol::open_inventory_packet(42).unwrap()
    );
    assert!(ledger.mark_transport_enqueued(40 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    ledger.apply(&InventoryEvent::Open(personal_open(7)));
    accept_cursor_move(&mut ledger, returning, true);
    assert!(!ledger.resync_required());
    assert!(ledger.begin_click(0).is_ok());
}

#[test]
fn legacy_return_admission_reconciles_a_waiting_reopen() {
    let mut ledger = ledger_with_slot_zero();
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Client));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.request_personal_close();
    ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.request_personal_open(42));
    // Both the return and the recovery open must leave without another input or timer.
    assert!(ledger.mark_transport_enqueued(30 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    assert!(ledger.mark_transport_enqueued(40 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
    ledger.apply(&InventoryEvent::Open(personal_open(7)));
    assert_eq!(ledger.displayed_stack(0).map(|stack| stack.count), Some(32));
    assert!(ledger.begin_click(0).is_ok());
}

#[test]
fn canceling_a_recovery_open_retains_its_earlier_cursor_return() {
    for (authority, sent) in [
        (InventoryAuthority::Server, false),
        (InventoryAuthority::Server, true),
        (InventoryAuthority::Client, false),
    ] {
        let mut ledger = ledger_with_slot_zero();
        ledger.apply(&InventoryEvent::Authority(authority));
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        let take = ledger.begin_click(0).unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        if authority == InventoryAuthority::Server {
            accept_cursor_move(&mut ledger, take, false);
        }
        ledger.request_personal_close();
        let returning = ledger.newest_request().unwrap().request_id;
        if sent {
            assert!(ledger.mark_transport_enqueued(30));
        }
        ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
        assert!(ledger.request_personal_open(42));
        ledger.request_personal_close();
        if !sent {
            assert!(ledger.mark_transport_enqueued(40 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
        }
        if authority == InventoryAuthority::Server {
            accept_cursor_move(&mut ledger, returning, true);
        }
        assert_eq!(ledger.displayed_stack(0).map(|stack| stack.count), Some(32));
        assert!(ledger.cursor_stack().is_none());
        assert!(!ledger.resync_required());
        assert!(ledger.request_personal_open(42));
        assert!(ledger.pending_batch().unwrap().is_some());
    }
}

#[test]
fn closing_a_recovery_open_returns_a_take_that_has_not_been_answered() {
    for accept_before_close in [false, true] {
        let mut ledger = ledger_with_slot_zero();
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        let take = ledger.begin_click(0).unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
        if accept_before_close {
            accept_cursor_move(&mut ledger, take, false);
        }
        ledger.request_personal_close();
        let returning = ledger.newest_request().unwrap().request_id;
        assert_ne!(returning, take);
        if !accept_before_close {
            accept_cursor_move(&mut ledger, take, false);
        }
        assert!(ledger.mark_transport_enqueued(30 + INVENTORY_REQUEST_TIMEOUT_MILLIS));
        accept_cursor_move(&mut ledger, returning, true);
        assert_eq!(ledger.displayed_stack(0).map(|stack| stack.count), Some(32));
        assert!(ledger.cursor_stack().is_none());
        assert!(!ledger.resync_required());
    }
}
