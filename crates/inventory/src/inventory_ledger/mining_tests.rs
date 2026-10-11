use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, CONTAINER_NAME_CURSOR, InventoryEvent,
    ItemStackResponseEvent, StackResponse, StackResponseSlot, StackResponseStatus,
};

use super::{
    INVENTORY_REQUEST_TIMEOUT_MILLIS, acknowledge_personal_open, correction, ledger_with_slot_zero,
};

/// Builds a server durability correction for the predicted inventory slot.
fn mining_response(
    request_id: i32,
    status: StackResponseStatus,
    damage: i32,
    stack_id: i32,
) -> InventoryEvent {
    let mut container = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 1, stack_id);
    container.slots = Arc::from([StackResponseSlot {
        durability_correction: damage,
        ..container.slots[0].clone()
    }]);
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status,
            request_id,
            containers: Arc::from([container]),
        }]),
    })
}

#[test]
fn mining_responses_correct_the_worn_slot_without_a_pending_gesture() {
    let mut ledger = ledger_with_slot_zero();
    let first = ledger.begin_mining_request(0, 4).unwrap();
    let second = ledger.begin_mining_request(0, 5).unwrap();
    assert_eq!(
        (first, second),
        (-3, -5),
        "mining ids share the gesture counter"
    );
    // Outstanding predictions chain onto each other.
    assert_eq!(ledger.predicted_slot_damage(0), Some(5));
    ledger.apply(&mining_response(
        first,
        StackResponseStatus::Accepted,
        4,
        77,
    ));
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(77)
    );
    assert_eq!(ledger.predicted_slot_damage(0), Some(5));
    // A rejected prediction is dropped; the last accepted damage remains.
    ledger.apply(&mining_response(
        second,
        StackResponseStatus::Rejected,
        9,
        88,
    ));
    assert_eq!(ledger.predicted_slot_damage(0), Some(4));
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(77)
    );
    // Unknown and repeated ids change nothing.
    ledger.apply(&mining_response(
        second,
        StackResponseStatus::Accepted,
        9,
        88,
    ));
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(77)
    );
    acknowledge_personal_open(&mut ledger, 2);
    assert_eq!(
        ledger.begin_click(0).unwrap(),
        -7,
        "the next gesture keeps a distinct id"
    );
}

#[test]
fn mining_corrections_never_touch_a_slot_owned_by_a_pending_gesture() {
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    let mining = ledger.begin_mining_request(0, 1).unwrap();
    let gesture = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.apply(&mining_response(
        mining,
        StackResponseStatus::Accepted,
        1,
        77,
    ));
    assert_eq!(ledger.pending_request_id(), Some(gesture));
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id: gesture,
            containers: Arc::from([
                correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, -1),
                correction(CONTAINER_NAME_CURSOR, 0, 32, 9),
            ]),
        }]),
    }));
    assert_eq!(ledger.pending_state(), None);
    assert!(!ledger.resync_required());
}

#[test]
fn outstanding_mining_requests_are_bounded() {
    let mut ledger = ledger_with_slot_zero();
    let ids = (0..40)
        .map(|damage| ledger.begin_mining_request(0, damage).unwrap())
        .collect::<Vec<_>>();
    ledger.apply(&mining_response(
        ids[0],
        StackResponseStatus::Accepted,
        0,
        55,
    ));
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(9),
        "an evicted request's late response is ignored"
    );
    assert_eq!(ledger.predicted_slot_damage(0), Some(39));
}

#[test]
fn cancelled_mining_request_leaves_no_prediction_behind() {
    let mut ledger = ledger_with_slot_zero();
    let id = ledger.begin_mining_request(0, 4).unwrap();
    ledger.cancel_mining_request(id);
    assert_eq!(ledger.predicted_slot_damage(0), None);
    assert_eq!(ledger.begin_mining_request(0, 4), Some(id - 2));
}

#[test]
fn accepted_mining_behind_an_expired_head_still_corrects_the_slot() {
    let mut ledger = ledger_with_slot_zero();
    let _unanswered = ledger.begin_mining_request(0, 4).unwrap();
    let answered = ledger.begin_mining_request(0, 5).unwrap();
    ledger.poll_timeout(0);
    ledger.apply(&mining_response(
        answered,
        StackResponseStatus::Accepted,
        5,
        88,
    ));
    ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1);
    assert_eq!(
        ledger
            .displayed_stack(0)
            .map(|stack| stack.stack_network_id),
        Some(88)
    );
}
