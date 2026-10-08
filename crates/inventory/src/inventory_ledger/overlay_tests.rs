//! Native sparse prediction and response contracts.

#[path = "overlay_tests/sparse.rs"]
mod sparse;

use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, CONTAINER_NAME_CURSOR, ContainerIdentity,
    ContainerOpenEvent, InventoryContentEvent, InventoryEvent, InventorySlotEvent,
    ItemStackResponseEvent, NetworkItemStack, SlotIdentity, StackRequestAction, StackResponse,
    StackResponseContainer, StackResponseSlot, StackResponseStatus,
};

use super::*;

fn stack(network_id: i32, stack_network_id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        network_id,
        count,
        stack_network_id,
        ..NetworkItemStack::default()
    }
}

fn player_content(slots: &[(usize, NetworkItemStack)]) -> InventoryEvent {
    let mut content = vec![NetworkItemStack::default(); PLAYER_INVENTORY_SLOT_COUNT];
    for (slot, stack) in slots {
        content[*slot] = stack.clone();
    }
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(0),
        slots: Arc::from(content),
        storage_item: NetworkItemStack::default(),
    })
}

fn cursor_content(stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: Some(-1),
            slot_type: Some(CONTAINER_NAME_CURSOR),
            dynamic_id: None,
        },
        slots: Arc::from([stack]),
        storage_item: NetworkItemStack::default(),
    })
}

fn open_ledger(slots: &[(usize, NetworkItemStack)]) -> PlayerInventoryLedger {
    let mut ledger = PlayerInventoryLedger::default();
    ledger.begin_session(1);
    ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
    ledger.apply(&player_content(slots));
    ledger.apply(&cursor_content(NetworkItemStack::default()));
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
    ledger
}

fn slot_push(slot: u16, stack: NetworkItemStack) -> InventoryEvent {
    InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot,
        },
        stack,
        storage_item: None,
    })
}

fn named(slot_type: u8) -> ContainerIdentity {
    ContainerIdentity {
        window_id: None,
        slot_type: Some(slot_type),
        dynamic_id: None,
    }
}

fn correction(slot_type: u8, slot: u8, count: u8, item_stack_id: i32) -> StackResponseContainer {
    StackResponseContainer {
        container: named(slot_type),
        slots: Arc::from([StackResponseSlot {
            slot,
            hotbar_slot: slot,
            count,
            item_stack_id,
            custom_name: Arc::from(""),
            filtered_custom_name: Arc::from(""),
            durability_correction: 0,
        }]),
    }
}

fn respond(
    ledger: &mut PlayerInventoryLedger,
    request_id: i32,
    status: StackResponseStatus,
    containers: Vec<StackResponseContainer>,
) {
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status,
            request_id,
            containers: Arc::from(containers),
        }]),
    }));
}

fn send_all(ledger: &mut PlayerInventoryLedger) {
    while ledger.pending_batch().unwrap().is_some() {
        assert!(ledger.mark_transport_enqueued(10));
    }
}

fn count(stack: Option<&NetworkItemStack>) -> Option<u16> {
    stack.map(|stack| stack.count)
}

/// Accepted corrections replace server counts and stack ids.
#[test]
fn accepted_response_applies_server_counts() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let request = ledger.begin_click(0).unwrap();
    send_all(&mut ledger);
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![
            correction(CONTAINER_NAME_CURSOR, 0, 2, 555),
            correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, 0),
        ],
    );
    let cursor = ledger.cursor_stack().unwrap();
    assert_eq!((cursor.count, cursor.stack_network_id), (2, 555));
    assert!(ledger.displayed_stack(0).is_none());
}

/// Gestures pipeline without waiting and reach the wire in creation order.
#[test]
fn gestures_pipeline_in_wire_order_over_the_folded_view() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4)), (1, stack(7, 11, 3))]);
    let first = ledger.begin_click(0).unwrap();
    let second = ledger.begin_click(5).unwrap();
    let third = ledger.begin_click(1).unwrap();
    assert_eq!((first, second, third), (-3, -5, -7));
    assert_eq!(ledger.pending_request_count(), 3);
    assert_eq!(count(ledger.displayed_stack(5)), Some(4));
    assert_eq!(count(ledger.cursor_stack()), Some(3));
    for expected in [first, second, third] {
        assert_eq!(
            ledger.first_unsent().map(|pending| pending.request_id),
            Some(expected)
        );
        assert!(ledger.mark_transport_enqueued(10));
    }
    assert_eq!(ledger.pending_batch().unwrap(), None);
    let StackRequestAction::Place { destination, .. } = ledger.queue[1].actions[0] else {
        panic!("second gesture places the held stack");
    };
    assert_eq!(destination.slot, 5);
}

/// A later acceptance updates backing cells immediately without erasing a
/// predecessor's historic snapshot.
#[test]
fn out_of_order_acceptance_settles_its_response_independently() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let head = ledger.begin_click(0).unwrap();
    let tail = ledger.begin_click(5).unwrap();
    send_all(&mut ledger);

    respond(
        &mut ledger,
        tail,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            5,
            4,
            777,
        )],
    );
    assert_eq!(ledger.pending_request_count(), 1);
    assert_eq!(ledger.pending_request_id(), Some(head));
    assert_eq!(count(ledger.confirmed_stack(Cell::Inventory(5))), Some(4));
    assert_eq!(count(ledger.displayed_stack(5)), Some(4));

    respond(
        &mut ledger,
        head,
        StackResponseStatus::Accepted,
        vec![
            correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, 0),
            correction(CONTAINER_NAME_CURSOR, 0, 4, 778),
        ],
    );
    assert_eq!(ledger.pending_request_count(), 0);
    let moved = ledger.displayed_stack(5).unwrap();
    assert_eq!((moved.count, moved.stack_network_id), (4, 777));
    assert!(ledger.displayed_stack(0).is_none());
    assert!(
        ledger.cursor_stack().is_none(),
        "already-removed newer owner cannot be reconciled from an old answer"
    );
    assert!(!ledger.resync_required());
}

/// A rejected tail is deleted at once without disturbing the head.
#[test]
fn out_of_order_rejection_deletes_only_that_request() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let head = ledger.begin_click(0).unwrap();
    let tail = ledger.begin_click(5).unwrap();
    send_all(&mut ledger);
    respond(&mut ledger, tail, StackResponseStatus::Rejected, Vec::new());
    assert_eq!(ledger.pending_request_id(), Some(head));
    assert_eq!(ledger.pending_request_count(), 1);
    assert!(ledger.displayed_stack(5).is_none());
    assert!(
        ledger.cursor_stack().is_none(),
        "rejection does not resurrect historic ownership"
    );
}

/// Servers may append ERROR and SUCCESS for one id; the first resolves it.
#[test]
fn duplicate_responses_for_one_request_resolve_once() {
    let original = stack(6, 10, 4);
    let mut ledger = open_ledger(&[(0, original.clone())]);
    let request = ledger.begin_click(0).unwrap();
    send_all(&mut ledger);
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([
            StackResponse {
                status: StackResponseStatus::Rejected,
                request_id: request,
                containers: Arc::from([]),
            },
            StackResponse {
                status: StackResponseStatus::Accepted,
                request_id: request,
                containers: Arc::from([correction(CONTAINER_NAME_CURSOR, 0, 4, 10)]),
            },
        ]),
    }));
    assert_eq!(ledger.pending_request_count(), 0);
    assert_eq!(ledger.displayed_stack(0), Some(&original));
    assert!(ledger.cursor_stack().is_none());
}

/// A backing push never changes an active absolute sparse prediction;
/// rejection uncovers exactly the pushed truth.
#[test]
fn server_push_of_same_stack_preserves_absolute_prediction() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let request = ledger.begin_take_count(0, 1).unwrap();
    send_all(&mut ledger);
    assert_eq!(count(ledger.displayed_stack(0)), Some(3));

    ledger.apply(&slot_push(0, stack(6, 10, 10)));
    assert_eq!(count(ledger.displayed_stack(0)), Some(3));

    respond(
        &mut ledger,
        request,
        StackResponseStatus::Rejected,
        Vec::new(),
    );
    assert_eq!(count(ledger.displayed_stack(0)), Some(10));
    assert!(ledger.cursor_stack().is_none());
}

/// Even a replacement backing item does not erase an active sparse cell.
#[test]
fn server_push_of_replacement_stack_preserves_sparse_cell_until_response() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let request = ledger.begin_take_count(0, 1).unwrap();
    send_all(&mut ledger);
    let replacement = stack(9, 99, 10);
    ledger.apply(&slot_push(0, replacement.clone()));
    assert_eq!(count(ledger.displayed_stack(0)), Some(3));
    assert_eq!(count(ledger.cursor_stack()), Some(1));

    respond(
        &mut ledger,
        request,
        StackResponseStatus::Rejected,
        Vec::new(),
    );
    assert_eq!(ledger.displayed_stack(0), Some(&replacement));
}

/// Authoritative slot updates during flight survive a later rejection.
#[test]
fn rejection_keeps_authoritative_slot_updates() {
    let mut ledger = open_ledger(&[(20, stack(6, 100, 3))]);
    let take = ledger.begin_click(20).unwrap();
    let place = ledger.begin_click(0).unwrap();
    send_all(&mut ledger);
    ledger.apply(&slot_push(0, stack(8, 555, 1)));
    ledger.apply(&slot_push(20, stack(6, 777, 3)));
    for request in [take, place] {
        respond(
            &mut ledger,
            request,
            StackResponseStatus::Rejected,
            Vec::new(),
        );
    }
    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, 555);
    assert_eq!(ledger.displayed_stack(20).unwrap().stack_network_id, 777);
    assert!(ledger.cursor_stack().is_none());
}

/// Without authoritative updates, rejection restores the pre-request state.
#[test]
fn rejection_restores_snapshot_without_authoritative_update() {
    let original = stack(6, 100, 3);
    let mut ledger = open_ledger(&[(20, original.clone())]);
    let take = ledger.begin_click(20).unwrap();
    let place = ledger.begin_click(0).unwrap();
    send_all(&mut ledger);
    respond(&mut ledger, take, StackResponseStatus::Rejected, Vec::new());
    respond(
        &mut ledger,
        place,
        StackResponseStatus::Rejected,
        Vec::new(),
    );
    assert_eq!(ledger.displayed_stack(20), Some(&original));
    assert!(ledger.displayed_stack(0).is_none());
}

/// A full content resend during flight is equally authoritative.
#[test]
fn rejection_keeps_authoritative_content_resend() {
    let mut ledger = open_ledger(&[(20, stack(6, 100, 3))]);
    let request = ledger.begin_click(20).unwrap();
    send_all(&mut ledger);
    ledger.apply(&player_content(&[(20, stack(6, 777, 3))]));
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Rejected,
        Vec::new(),
    );
    assert_eq!(ledger.displayed_stack(20).unwrap().stack_network_id, 777);
    assert!(ledger.displayed_stack(0).is_none());
}

/// Overlapping queued transfers that are both rejected restore the source.
#[test]
fn rejection_only_clears_answered_request_ownership() {
    let original = stack(6, 10, 2);
    let mut ledger = open_ledger(&[(0, original.clone())]);
    let first = ledger.begin_click(0).unwrap();
    let second = ledger.begin_place_count(3, 1).unwrap();
    assert_eq!(count(ledger.displayed_stack(3)), Some(1));
    send_all(&mut ledger);
    respond(
        &mut ledger,
        first,
        StackResponseStatus::Rejected,
        Vec::new(),
    );
    assert_eq!(
        count(ledger.displayed_stack(3)),
        Some(1),
        "later sparse ownership remains until its own response"
    );
    respond(
        &mut ledger,
        second,
        StackResponseStatus::Rejected,
        Vec::new(),
    );
    assert_eq!(ledger.displayed_stack(0), Some(&original));
    assert!(ledger.displayed_stack(3).is_none());
    assert!(ledger.cursor_stack().is_none());
}

/// A server that empties the cursor before accepting a placement still gets
/// the predicted item restored under the accepted count and id.
#[test]
fn accepted_placement_after_cursor_content_restores_item() {
    for accepted in [true, false] {
        let mut ledger = open_ledger(&[]);
        let held = stack(6, 10, 1);
        ledger.apply(&cursor_content(held.clone()));
        let request = ledger.begin_click(20).unwrap();
        send_all(&mut ledger);
        ledger.apply(&cursor_content(NetworkItemStack::default()));
        let status = if accepted {
            StackResponseStatus::Accepted
        } else {
            StackResponseStatus::Rejected
        };
        respond(
            &mut ledger,
            request,
            status,
            vec![
                correction(CONTAINER_NAME_CURSOR, 0, 0, 0),
                correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 20, 1, 11),
            ],
        );
        let slot = ledger.displayed_stack(20);
        if accepted {
            let slot = slot.expect("accepted destination keeps its item");
            assert_eq!(
                (slot.network_id, slot.count, slot.stack_network_id),
                (6, 1, 11)
            );
        } else {
            assert!(slot.is_none(), "a rejected placement resurrects nothing");
        }
        assert!(ledger.cursor_stack().is_none());
    }
}

/// The pipeline is bounded; each answered request frees its own capacity.
#[test]
fn request_queue_bounds_and_recovers() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let mut requests = Vec::new();
    for index in 0..MAX_PENDING_REQUESTS {
        let slot = if matches!(index % 4, 0 | 3) { 0 } else { 1 };
        requests.push(ledger.begin_click(slot).unwrap());
    }
    assert_eq!(ledger.begin_click(0), Err(InventoryGestureError::Busy));
    send_all(&mut ledger);
    for request in &requests[1..] {
        respond(
            &mut ledger,
            *request,
            StackResponseStatus::Accepted,
            Vec::new(),
        );
    }
    assert_eq!(ledger.pending_request_count(), 1);

    respond(
        &mut ledger,
        requests[0],
        StackResponseStatus::Accepted,
        Vec::new(),
    );
    assert_eq!(ledger.pending_request_count(), 0);
    assert_eq!(count(ledger.displayed_stack(0)), Some(4));
    assert!(ledger.cursor_stack().is_none());
    assert!(!ledger.resync_required());
    assert!(ledger.begin_click(0).is_ok());
}

/// Queue pressure rolls back every unsent request but keeps admitted ones.
#[test]
fn transport_pressure_drops_only_unsent_requests() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let admitted = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.begin_click(5).unwrap();
    ledger.begin_click(5).unwrap();
    ledger.note_transport_pressure(20);
    ledger.note_transport_pressure(20 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert_eq!(ledger.pending_request_count(), 1);
    assert_eq!(ledger.pending_request_id(), Some(admitted));
    assert!(
        ledger.cursor_stack().is_none(),
        "discarded latest unsent owner does not resurrect an older snapshot"
    );
    assert!(!ledger.resync_required());
}

/// A timed-out request is retired once complete content refreshes every
/// surface it touched; nothing rolls back before that.
#[test]
fn timeout_retires_only_after_full_refresh() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    ledger.begin_click(0).unwrap();
    send_all(&mut ledger);
    ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert_eq!(ledger.pending_request_count(), 1);
    assert_eq!(
        ledger.begin_click(1),
        Err(InventoryGestureError::ResyncRequired)
    );

    ledger.apply(&player_content(&[(0, stack(6, 10, 4))]));
    assert_eq!(
        ledger.pending_request_count(),
        1,
        "the cursor is still unrefreshed"
    );
    ledger.apply(&cursor_content(NetworkItemStack::default()));
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(!ledger.resync_required());
    assert_eq!(count(ledger.displayed_stack(0)), Some(4));
    assert!(ledger.cursor_stack().is_none());
}

/// A mining request rides player input: it is admitted at once, sits ahead
/// of unsent gestures, never counts as a gesture, and its timeout never
/// blocks inventory gestures.
#[test]
fn mining_requests_queue_at_their_wire_position_without_locking() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let admitted = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    let unsent = ledger.begin_click(5).unwrap();
    let mining = ledger.begin_mining_request(1, 7).unwrap();
    assert_eq!((admitted, unsent, mining), (-3, -5, -7));
    let order: Vec<i32> = ledger
        .queue
        .iter()
        .map(|pending| pending.request_id)
        .collect();
    assert_eq!(order, [admitted, mining, unsent]);
    assert_eq!(ledger.pending_request_count(), 2);
    assert_eq!(ledger.first_unsent().unwrap().request_id, unsent);
    assert_eq!(ledger.predicted_slot_damage(1), Some(7));

    ledger.poll_timeout(10);
    ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert_eq!(
        ledger.predicted_slot_damage(1),
        None,
        "an unanswered break is forgotten"
    );
    assert!(ledger.queue.iter().all(|pending| pending.mining.is_none()));
    assert!(
        !ledger.surface_flagged(CellSurface::Crafting)
            && !ledger.surface_flagged(CellSurface::Armor)
    );
}

/// An accepted mining response settles through the shared path, carrying its
/// durability correction onto the slot overlay.
#[test]
fn accepted_mining_settles_durability_through_the_queue() {
    let mut ledger = open_ledger(&[(2, stack(6, 10, 1))]);
    let mining = ledger.begin_mining_request(2, 3).unwrap();
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id: mining,
            containers: Arc::from([StackResponseContainer {
                container: named(protocol::CONTAINER_NAME_HOTBAR),
                slots: Arc::from([StackResponseSlot {
                    slot: 2,
                    hotbar_slot: 2,
                    count: 1,
                    item_stack_id: 11,
                    custom_name: Arc::from(""),
                    filtered_custom_name: Arc::from(""),
                    durability_correction: 3,
                }]),
            }]),
        }]),
    }));
    assert!(ledger.queue.is_empty());
    assert_eq!(ledger.displayed_stack(2).unwrap().stack_network_id, 11);
    assert_eq!(ledger.predicted_slot_damage(2), Some(3));
}

/// A full queue sends the break without a request instead of refusing it.
#[test]
fn full_queue_sends_mining_without_a_request() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    for index in 0..MAX_PENDING_REQUESTS {
        let slot = if matches!(index % 4, 0 | 3) { 0 } else { 1 };
        ledger.begin_click(slot).unwrap();
    }
    assert_eq!(ledger.begin_mining_request(0, 1), None);
    assert_eq!(ledger.queue.len(), MAX_PENDING_REQUESTS);
}

/// An answered request is retired immediately; an unanswered request's
/// timeout gates only the surfaces it touched.
#[test]
fn timeouts_skip_accepted_requests_and_gate_only_their_surfaces() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4)), (20, stack(7, 12, 1))]);
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
        slots: Arc::from([stack(8, 30, 2)]),
        storage_item: NetworkItemStack::default(),
    }));
    let head = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Offhand), Some(1))
        .unwrap();
    let tail = ledger.begin_click(0).unwrap();
    send_all(&mut ledger);
    respond(&mut ledger, tail, StackResponseStatus::Accepted, Vec::new());
    ledger.poll_timeout(10 + INVENTORY_REQUEST_TIMEOUT_MILLIS);
    assert!(ledger.queue[0].timed_out);
    assert_eq!(
        ledger.queue.len(),
        1,
        "accepted requests never wait behind a head"
    );
    assert!(ledger.begin_quick_move(InventoryTarget::Player(20)).is_ok());
    assert_eq!(
        ledger.begin_drop(DropSource::Target(InventoryTarget::Offhand), Some(1)),
        Err(InventoryGestureError::ResyncRequired),
        "the offhand is still unverified"
    );
    respond(&mut ledger, head, StackResponseStatus::Accepted, Vec::new());
    assert!(!ledger.resync_required());
}
