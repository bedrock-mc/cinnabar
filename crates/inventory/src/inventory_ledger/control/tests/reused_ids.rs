use super::*;
use crate::inventory_ledger::lifecycle_tests::accept_cursor_move;

#[test]
fn old_close_completion_preserves_a_replacement_and_its_items() {
    for old_storage in [false, true] {
        for new_storage in [false, true] {
            for expire in [false, true] {
                let mut ledger = ledger_with_slot_zero();
                if old_storage {
                    ledger.apply(&storage_open(3));
                    ledger.request_storage_close();
                } else {
                    ledger.apply(&InventoryEvent::Open(personal_open(3)));
                    ledger.request_personal_close();
                }
                assert!(ledger.mark_transport_enqueued(0));
                if new_storage {
                    ledger.apply(&storage_open(3));
                } else {
                    ledger.apply(&InventoryEvent::Open(personal_open(3)));
                }
                let generation = ledger.storage_generation();
                let request = ledger.begin_click(0).unwrap();
                assert!(ledger.mark_transport_enqueued(20));
                if expire {
                    ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
                } else {
                    let kind = if old_storage {
                        GENERIC_STORAGE_WINDOW_TYPE
                    } else {
                        crate::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE
                    };
                    acknowledge_close(&mut ledger, 3, kind);
                }
                assert_eq!(ledger.storage_generation(), generation);
                assert_eq!(ledger.pending_request_id(), Some(request));
                assert!(ledger.pending_control_packet().unwrap().is_none());
                accept_cursor_move(&mut ledger, request, false);
                assert_eq!(ledger.pending_request_id(), None);
                assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(32));
            }
        }
    }
}

#[test]
fn a_replacement_close_waits_for_an_admitted_close_with_the_same_id() {
    let mut ledger = ledger_with_slot_zero();
    ledger.apply(&InventoryEvent::Open(personal_open(3)));
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&storage_open(3));
    ledger.request_storage_close();
    assert!(ledger.pending_control_packet().unwrap().is_none());
    acknowledge_close(
        &mut ledger,
        3,
        crate::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
    );
    assert_eq!(
        ledger.pending_control_packet().unwrap().unwrap(),
        protocol::container_close_packet(3).unwrap()
    );
    assert!(ledger.mark_transport_enqueued(1));
    acknowledge_close(&mut ledger, 3, GENERIC_STORAGE_WINDOW_TYPE);
    assert!(ledger.pending_batch().unwrap().is_none());
    assert!(ledger.request_personal_open(42));
}
