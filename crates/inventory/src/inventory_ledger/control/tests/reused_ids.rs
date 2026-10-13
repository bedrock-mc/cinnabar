use super::*;
use crate::inventory_ledger::lifecycle_tests::accept_cursor_move;

/// Applies a server command without consuming a separately delayed client acknowledgement.
fn server_close(ledger: &mut PlayerInventoryLedger, window_id: i32, window_type: i8) {
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(window_id),
        window_type,
        server_initiated: true,
    }));
}

#[test]
fn old_storage_completion_preserves_a_new_personal_generation_and_its_items() {
    for expire in [false, true] {
        for covered_personal in [false, true] {
            let mut ledger = ledger_with_slot_zero();
            let mut server = Server {
                personal_window_id: 3,
                ..Server::default()
            };
            if covered_personal {
                assert!(ledger.request_personal_open(42));
                server.send(&mut ledger, 0, false);
            }
            server.open_storage(&mut ledger, 3);
            if covered_personal {
                server.deliver(&mut ledger);
            }
            ledger.request_storage_close();
            server.send(&mut ledger, 0, false);
            server_close(&mut ledger, 3, GENERIC_STORAGE_WINDOW_TYPE);
            if covered_personal {
                server_close(&mut ledger, 3, PERSONAL_INVENTORY_WINDOW_TYPE);
            }
            assert!(ledger.request_personal_open(42));
            server.send(&mut ledger, 10, false);
            ledger.apply(&server.replies.pop_back().unwrap());
            let personal = ledger.controls.personal.confirmed.unwrap();
            server_close(&mut ledger, 3, GENERIC_STORAGE_WINDOW_TYPE);
            assert_eq!(ledger.controls.personal.confirmed, Some(personal));
            let request = ledger.begin_click(0).unwrap();
            assert!(ledger.mark_transport_enqueued(20));

            if expire {
                ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
            } else {
                server.deliver(&mut ledger);
            }
            assert_eq!(ledger.controls.personal.confirmed, Some(personal));
            assert!(ledger.pending_control().is_none(), "no duplicate open");
            assert_eq!(ledger.pending_request_id(), Some(request));
            accept_cursor_move(&mut ledger, request, false);
            assert_eq!(ledger.pending_request_id(), None);
            assert!(ledger.displayed_stack(0).is_none());
            assert_eq!(ledger.cursor_stack().map(|stack| stack.count), Some(32));
            assert!(server.personal_open);
            assert_eq!(server.duplicate_opens, 0);
        }
    }
}

#[test]
fn one_storage_close_covers_late_personal_cleanup_without_closing_the_replacement() {
    for expire in [false, true] {
        let mut ledger = ledger_with_slot_zero();
        let mut server = Server {
            personal_window_id: 3,
            ..Server::default()
        };
        assert!(ledger.request_personal_open(42));
        server.send(&mut ledger, 0, false);
        server.open_storage(&mut ledger, 3);
        ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
        server.deliver(&mut ledger);
        assert!(ledger.pending_control().is_none());

        ledger.request_storage_close();
        server.replace_next_storage_close = true;
        server.send(&mut ledger, INVENTORY_REQUEST_TIMEOUT_MILLIS, expire);
        assert!(ledger.storage.is_none());
        assert!(
            ledger.pending_control().is_none(),
            "the same ID already has an outstanding close"
        );
        server.send(&mut ledger, INVENTORY_REQUEST_TIMEOUT_MILLIS, false);
        assert_eq!(server.closes, 1);

        // A replacement can arrive before the old close completes, even when its reply is lost.
        ledger.apply(&server.replies.pop_back().unwrap());
        let replacement = ledger.controls.storage.confirmed.unwrap();
        if expire {
            ledger.poll_timeout(2 * INVENTORY_REQUEST_TIMEOUT_MILLIS);
        } else {
            server.deliver(&mut ledger);
        }
        assert_eq!(ledger.controls.storage.confirmed, Some(replacement));
        assert_eq!(ledger.storage_generation(), Some(replacement.generation));
        assert!(ledger.controls.personal.confirmed.is_none());
        assert!(!ledger.controls.personal.reset_required);
        assert!(ledger.pending_control().is_none());
        server.send(&mut ledger, 2 * INVENTORY_REQUEST_TIMEOUT_MILLIS, false);
        assert_eq!(server.closes, 1);
        assert_eq!(server.storage, Some(3));
    }
}

#[test]
fn a_late_storage_reply_cannot_release_a_personal_close_reusing_its_id() {
    let mut ledger = ledger_with_slot_zero();
    let mut server = Server {
        personal_window_id: 3,
        ..Server::default()
    };
    server.open_storage(&mut ledger, 3);
    ledger.request_storage_close();
    server.send(&mut ledger, 0, false);
    ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.request_personal_open(42));
    server.send(&mut ledger, INVENTORY_REQUEST_TIMEOUT_MILLIS, false);
    ledger.apply(&server.replies.pop_back().unwrap());
    ledger.request_personal_close();
    server.send(&mut ledger, INVENTORY_REQUEST_TIMEOUT_MILLIS, false);
    let personal = ledger.controls.personal.confirmed;
    server.deliver(&mut ledger);
    assert_eq!(ledger.controls.personal.confirmed, personal);
    assert!(ledger.controls.personal.sent.is_some());
    server.deliver(&mut ledger);
    assert!(ledger.controls.personal.confirmed.is_none());
    assert!(ledger.controls.personal.sent.is_none());
}

#[test]
fn a_storage_close_waits_for_a_personal_close_with_the_same_id() {
    let mut ledger = ledger_with_slot_zero();
    let mut server = Server {
        personal_window_id: 3,
        ..Server::default()
    };
    assert!(ledger.request_personal_open(42));
    server.send(&mut ledger, 0, false);
    server.deliver(&mut ledger);
    ledger.request_personal_close();
    server.send(&mut ledger, 10, false);
    server_close(&mut ledger, 3, PERSONAL_INVENTORY_WINDOW_TYPE);
    server.open_storage(&mut ledger, 3);
    let storage = ledger.controls.storage.confirmed;
    ledger.request_storage_close();
    assert!(ledger.pending_control().is_none());
    server.send(&mut ledger, 20, false);
    assert_eq!(server.closes, 1);
    server.deliver(&mut ledger);
    assert_eq!(ledger.controls.storage.confirmed, storage);
    server.send(&mut ledger, 30, false);
    assert_eq!(server.closes, 2);
    server.deliver(&mut ledger);
    assert!(ledger.controls.storage.confirmed.is_none());
}

#[test]
fn a_covering_storage_close_requires_a_fresh_personal_open_before_gestures() {
    for reply_before_close in [false, true] {
        for reopen_before_reply in [false, true] {
            for expire in [false, true] {
                for reopen_before_close in [false, true] {
                    covering_storage_close_interleaving(
                        reply_before_close,
                        reopen_before_reply,
                        expire,
                        reopen_before_close,
                    );
                }
            }
        }
    }
}

/// Checks delayed identity, gesture admission, and retained intent for one same-ID ordering.
pub(super) fn covering_storage_close_interleaving(
    reply_before_close: bool,
    reopen_before_reply: bool,
    expire: bool,
    reopen_before_close: bool,
) {
    let mut ledger = ledger_with_slot_zero();
    let mut server = Server {
        personal_window_id: 3,
        ..Server::default()
    };
    assert!(ledger.request_personal_open(42));
    server.send(&mut ledger, 0, false);
    server.open_storage(&mut ledger, 3);
    if reply_before_close {
        server.deliver(&mut ledger);
    }
    ledger.request_storage_close();
    if reopen_before_close {
        assert!(ledger.request_personal_open(42));
    }
    server.send(&mut ledger, 10, expire);
    assert!(!server.personal_open);
    if reopen_before_reply {
        assert!(ledger.request_personal_open(42));
    }
    if !reply_before_close {
        server.deliver(&mut ledger);
    }
    if !reopen_before_reply {
        assert!(ledger.request_personal_open(42));
    }
    assert!(ledger.pending_control().is_none());
    assert_eq!(
        ledger.begin_click(0),
        Err(crate::inventory_ledger::InventoryGestureError::PersonalInventoryUnavailable)
    );
    assert_eq!(ledger.pending_request_id(), None);
    assert_eq!(ledger.displayed_stack(0).map(|stack| stack.count), Some(32));
    assert!(ledger.cursor_stack().is_none());

    let now = 10 + INVENTORY_REQUEST_TIMEOUT_MILLIS;
    if expire {
        ledger.poll_timeout(now);
    } else {
        server.deliver(&mut ledger);
    }
    assert!(ledger.personal_inventory_desired_open());
    assert!(ledger.controls.personal.confirmed.is_none());
    assert!(matches!(
        ledger.pending_control(),
        Some(ControlRequest::Open { .. })
    ));
    assert_eq!(
        ledger.begin_click(0),
        Err(crate::inventory_ledger::InventoryGestureError::PersonalInventoryUnavailable)
    );
    server.send(&mut ledger, now, false);
    server.deliver(&mut ledger);
    assert!(server.personal_open);
    assert!(ledger.begin_click(0).is_ok());
    assert_eq!(server.closes, 1);
    assert_eq!(server.duplicate_opens, 0);
}

#[test]
fn a_storage_close_preserves_a_delayed_personal_reply_with_a_different_id() {
    let mut ledger = ledger_with_slot_zero();
    let mut server = Server::default();
    assert!(ledger.request_personal_open(42));
    server.send(&mut ledger, 0, false);
    server.open_storage(&mut ledger, 3);
    ledger.request_storage_close();
    server.send(&mut ledger, 10, false);
    assert!(ledger.request_personal_open(42));
    server.deliver(&mut ledger);
    let personal = ledger.controls.personal.confirmed.unwrap();
    server.deliver(&mut ledger);
    assert_eq!(ledger.controls.personal.confirmed, Some(personal));
    assert!(ledger.pending_control().is_none());
    assert!(ledger.begin_click(0).is_ok());
    assert!(server.personal_open);
    assert_eq!(server.duplicate_opens, 0);
}
