use super::*;
use crate::inventory_ledger::{
    PERSONAL_INVENTORY_WINDOW_TYPE, lifecycle_tests::accept_cursor_move,
};

/// Opens a personal replacement while a prior personal close still awaits its answer.
fn replacement_behind_close(replacement_id: i32) -> PlayerInventoryLedger {
    let mut ledger = ledger_with_slot_zero();
    ledger.apply(&InventoryEvent::Open(personal_open(3)));
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(personal_open(replacement_id)));
    ledger
}

#[test]
fn repeated_personal_replacement_keeps_its_item_requests() {
    for replacement_id in [3, 7] {
        for admitted in [false, true] {
            for expire in [false, true] {
                let mut ledger = replacement_behind_close(replacement_id);
                let request = ledger.begin_click(0).unwrap();
                if admitted {
                    assert!(ledger.mark_transport_enqueued(20));
                }
                ledger.apply(&InventoryEvent::Open(personal_open(replacement_id)));
                assert_eq!(ledger.pending_request_id(), Some(request));
                assert!(!ledger.resync_required());
                assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(32));
                if expire {
                    ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
                } else {
                    acknowledge_close(&mut ledger, 3, PERSONAL_INVENTORY_WINDOW_TYPE);
                }
                if !admitted {
                    assert!(ledger.mark_transport_enqueued(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1));
                }
                accept_cursor_move(&mut ledger, request, false);
                assert_eq!(ledger.pending_request_count(), 0);
                assert!(!ledger.resync_required());
                assert!(ledger.begin_click(0).is_ok());
            }
        }
    }
}

#[test]
fn closing_a_personal_replacement_allows_storage_before_the_older_close_settles() {
    for replacement_id in [3, 7] {
        for expire in [false, true] {
            let mut ledger = replacement_behind_close(replacement_id);
            ledger.request_personal_close();
            assert!(!ledger.personal_inventory_desired_open());
            assert!(ledger.pending_batch().unwrap().is_none());
            ledger.apply(&storage_open(replacement_id));
            assert_eq!(ledger.window_kind(), Some(protocol::WindowKind::Storage));
            assert!(!ledger.personal_inventory_desired_open());
            let request = ledger.begin_click(0).unwrap();
            assert!(ledger.mark_transport_enqueued(20));
            if expire {
                ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
            } else {
                acknowledge_close(&mut ledger, 3, PERSONAL_INVENTORY_WINDOW_TYPE);
            }
            accept_cursor_move(&mut ledger, request, false);
            assert_eq!(ledger.window_kind(), Some(protocol::WindowKind::Storage));
            assert_eq!(ledger.pending_request_count(), 0);
            assert!(!ledger.resync_required());
            assert!(ledger.begin_click(0).is_ok());
        }
    }
}

#[test]
fn closing_replacements_retire_returns_sent_before_their_close_can_leave() {
    for storage in [false, true] {
        for replacement_id in [3, 7] {
            for close_first in [false, true] {
                let mut ledger = ledger_with_slot_zero();
                let open = |window_id| {
                    if storage {
                        storage_open(window_id)
                    } else {
                        InventoryEvent::Open(personal_open(window_id))
                    }
                };
                let close = |ledger: &mut PlayerInventoryLedger| {
                    if storage {
                        ledger.request_storage_close();
                    } else {
                        ledger.request_personal_close();
                    }
                };
                let window_type = if storage {
                    GENERIC_STORAGE_WINDOW_TYPE
                } else {
                    PERSONAL_INVENTORY_WINDOW_TYPE
                };
                ledger.apply(&open(3));
                close(&mut ledger);
                assert!(ledger.mark_transport_enqueued(0));
                ledger.apply(&open(replacement_id));
                let take = ledger.begin_click(0).unwrap();
                assert!(ledger.mark_transport_enqueued(20));
                accept_cursor_move(&mut ledger, take, false);
                close(&mut ledger);
                let returning = ledger.newest_request().unwrap().request_id;
                assert!(ledger.mark_transport_enqueued(21));
                assert!(ledger.pending_batch().unwrap().is_none());
                let replacement = if storage {
                    InventoryEvent::Open(personal_open(9))
                } else {
                    storage_open(9)
                };
                ledger.apply(&replacement);
                assert_eq!(
                    ledger.window_kind(),
                    (!storage).then_some(protocol::WindowKind::Storage)
                );
                if close_first {
                    acknowledge_close(&mut ledger, 3, window_type);
                }
                accept_cursor_move(&mut ledger, returning, true);
                if !close_first {
                    acknowledge_close(&mut ledger, 3, window_type);
                }
                assert_eq!(ledger.pending_request_count(), 0);
                assert!(!ledger.resync_required());
                assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(32));
                assert!(ledger.displayed_stack(0).is_none());
                assert!(ledger.begin_click(0).is_ok());
            }
        }
    }
}
