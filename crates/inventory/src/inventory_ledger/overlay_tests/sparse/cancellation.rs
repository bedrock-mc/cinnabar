//! Cancelling untransported gestures preserves their predecessor's visible ownership.

use super::*;

/// Cancelling a placement also cancels dependent gestures and restores the original pickup.
#[test]
fn cancelling_unsent_suffix_restores_the_surviving_pickup() {
    let original = stack(6, 10, 4);
    let other = stack(7, 11, 3);
    let mut ledger = open_ledger(&[(0, original), (1, other.clone())]);
    let take = ledger.begin_click(0).unwrap();
    let place = ledger.begin_click(5).unwrap();
    ledger.begin_click(1).unwrap();

    ledger.abandon_requests(|request| request.request_id == place);

    assert_eq!(ledger.pending_request_id(), Some(take));
    assert_eq!(ledger.pending_request_count(), 1);
    assert_eq!(ledger.cursor_stack().unwrap().count, 4);
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, take);
    assert!(ledger.displayed_stack(0).is_none());
    assert!(ledger.displayed_stack(5).is_none());
    assert_eq!(ledger.displayed_stack(1), Some(&other));
    assert!(!ledger.resync_required());
}

/// Empty predictions retain ownership when a later untransported pickup is cancelled.
#[test]
fn cancelling_unsent_pickup_restores_the_surviving_empty_cursor() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4)), (1, stack(7, 11, 3))]);
    let original = ledger.begin_click(0).unwrap();
    let place = ledger.begin_click(5).unwrap();
    let take = ledger.begin_click(1).unwrap();

    ledger.abandon_requests(|request| request.request_id == take);

    assert_eq!(ledger.pending_request_count(), 2);
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.displayed_stack(5).unwrap().count, 4);
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 3);
    send_all(&mut ledger);
    respond(
        &mut ledger,
        original,
        StackResponseStatus::Accepted,
        vec![correction(CONTAINER_NAME_CURSOR, 0, 4, 91)],
    );
    assert_eq!(ledger.pending_request_id(), Some(place));
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.displayed_stack(5).unwrap().count, 4);
    assert!(!ledger.resync_required());
}

/// Local cancellation does not resurrect history already hidden by a server rejection.
#[test]
fn cancelling_unsent_pickup_does_not_restore_an_inactive_historic_cursor() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4)), (1, stack(7, 11, 3))]);
    let original = ledger.begin_click(0).unwrap();
    let drop = ledger.begin_drop(DropSource::Cursor, None).unwrap();
    send_all(&mut ledger);
    respond(&mut ledger, drop, StackResponseStatus::Rejected, vec![]);
    let later = ledger.begin_click(1).unwrap();

    ledger.abandon_requests(|request| request.request_id == later);

    assert_eq!(ledger.pending_request_id(), Some(original));
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 3);
    assert!(!ledger.resync_required());
}
