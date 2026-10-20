use super::*;

#[test]
fn missing_personal_open_ack_keeps_the_cursor_return_owned() {
    for (reopen, late_ack) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut ledger = ledger_with_slot_zero();
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        let take = ledger.begin_click(0).unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        accept_cursor_move(&mut ledger, take, false);
        ledger.request_personal_close();
        let returning = ledger.newest_request().expect("cursor return").request_id;
        assert!(ledger.mark_transport_enqueued(30));
        ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
        if reopen {
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
