use std::sync::Arc;

use protocol::{
    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, CONTAINER_NAME_CURSOR, InventoryEvent,
    ItemStackResponseEvent, StackResponse, StackResponseSlot, StackResponseStatus,
};

use super::{
    INVENTORY_REQUEST_TIMEOUT_MILLIS, acknowledge_personal_open, correction, ledger_with_slot_zero,
};

/// Builds the exact old and new descriptors for one admitted hotbar tool change.
fn destroy_change(
    ledger: &super::PlayerInventoryLedger,
    damage: i32,
    maximum: i32,
) -> protocol::PredictedSlotChange {
    let stack = ledger.backing_stack(0).unwrap().clone();
    let from =
        protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
    let legacy_request_id = protocol::next_legacy_request_id().unwrap();
    protocol::PredictedSlotChange {
        to: from.worn(damage, Some(maximum), legacy_request_id).unwrap(),
        from,
        legacy_request_id,
    }
}

#[test]
fn admitted_client_tool_wear_updates_backing_and_correlates_legacy_replies() {
    let mut ledger = ledger_with_slot_zero();
    let first = destroy_change(&ledger, 4, 250);
    assert!(ledger.settle_destroy_slot_change(0, &first));
    assert!(
        !ledger.settle_destroy_slot_change(0, &first),
        "duplicate admission"
    );
    assert_eq!(
        ledger.displayed_stack(0).unwrap().stack_network_id,
        first.legacy_request_id
    );
    assert_eq!(
        protocol::item_extra_damage(&ledger.displayed_stack(0).unwrap().extra_data),
        Some(4)
    );
    ledger.apply(&mining_response(
        first.legacy_request_id,
        StackResponseStatus::Accepted,
        5,
        77,
    ));
    let corrected = ledger.displayed_stack(0).unwrap().clone();
    assert_eq!(corrected.stack_network_id, 77);
    assert_eq!(protocol::item_extra_damage(&corrected.extra_data), Some(5));
    let second = destroy_change(&ledger, 7, 250);
    assert!(ledger.settle_destroy_slot_change(0, &second));
    ledger.apply(&mining_response(
        second.legacy_request_id,
        StackResponseStatus::Rejected,
        0,
        0,
    ));
    assert_eq!(ledger.displayed_stack(0), Some(&corrected));
}

#[test]
fn a_client_broken_tool_stays_empty_after_an_older_reply_and_a_replacement_wins() {
    let mut ledger = ledger_with_slot_zero();
    // The fixture starts with a stack; a damageable single item occupies the same cell.
    let mut stack = ledger.displayed_stack(0).unwrap().clone();
    stack.count = 1;
    let event = protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: protocol::ContainerIdentity::window(0),
            slot: 0,
        },
        stack,
        storage_item: None,
    };
    ledger.apply(&InventoryEvent::Slot(event.clone()));
    let worn = destroy_change(&ledger, 250, 250);
    assert!(ledger.settle_destroy_slot_change(0, &worn));
    let broken = destroy_change(&ledger, 252, 250);
    assert!(broken.to.is_empty());
    assert!(ledger.settle_destroy_slot_change(0, &broken));
    assert!(ledger.displayed_stack(0).is_none());
    ledger.apply(&mining_response(
        worn.legacy_request_id,
        StackResponseStatus::Accepted,
        250,
        77,
    ));
    assert!(ledger.displayed_stack(0).is_none());
    ledger.apply(&InventoryEvent::Slot(event.clone()));
    ledger.apply(&mining_response(
        broken.legacy_request_id,
        StackResponseStatus::Rejected,
        0,
        0,
    ));
    assert_eq!(ledger.displayed_stack(0), Some(&event.stack));
}

#[test]
fn retiring_client_wear_history_keeps_the_latest_backing_damage() {
    let mut ledger = ledger_with_slot_zero();
    for damage in 1..=40 {
        let change = destroy_change(&ledger, damage, 250);
        assert!(ledger.settle_destroy_slot_change(0, &change));
    }
    ledger.poll_timeout(0);
    ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS + 1);
    assert_eq!(ledger.predicted_slot_damage(0), Some(40));
    assert_eq!(
        protocol::item_extra_damage(&ledger.displayed_stack(0).unwrap().extra_data),
        Some(40)
    );
}

#[test]
fn an_accepted_earlier_pickup_retires_client_wear_without_duplicating_the_tool() {
    for damage in [6, 251] {
        let mut ledger = ledger_with_slot_zero();
        let mut stack = ledger.backing_stack(0).unwrap().clone();
        stack.count = 1;
        ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
            identity: protocol::SlotIdentity {
                container: protocol::ContainerIdentity::window(0),
                slot: 0,
            },
            stack,
            storage_item: None,
        }));
        acknowledge_personal_open(&mut ledger, 2);
        let pickup = ledger.begin_click(0).unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        let change = destroy_change(&ledger, damage, 250);
        assert!(ledger.settle_destroy_slot_change(0, &change));
        ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
            responses: [StackResponse {
                status: StackResponseStatus::Accepted,
                request_id: pickup,
                containers: [
                    correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, 0),
                    correction(CONTAINER_NAME_CURSOR, 0, 1, 77),
                ]
                .into(),
            }]
            .into(),
        }));
        assert!(ledger.backing_stack(0).is_none());
        assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 77);
        ledger.apply(&mining_response(
            change.legacy_request_id,
            StackResponseStatus::Accepted,
            damage,
            88,
        ));
        assert!(ledger.backing_stack(0).is_none());
        assert_eq!(ledger.cursor_stack().unwrap().count, 1);
    }
}

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

#[test]
fn corrected_stack_identity_survives_responses_and_moves_but_not_replacement() {
    use super::{CellGesture, InventoryTarget};
    let mut ledger = ledger_with_slot_zero();
    let source = InventoryTarget::Player(0);
    for (damage, stack_id) in [(4, 77), (5, 88)] {
        let request = ledger.begin_mining_request(0, damage).unwrap();
        ledger.apply(&mining_response(
            request,
            StackResponseStatus::Accepted,
            damage,
            stack_id,
        ));
        assert!(ledger.target_retains_stack_id(source, 9));
        assert_eq!(
            ledger.displayed_stack(0).unwrap().stack_network_id,
            stack_id
        );
    }
    acknowledge_personal_open(&mut ledger, 2);
    ledger.begin_click(0).unwrap();
    assert_eq!(ledger.cursor_stack().unwrap().count, 1);
    let destination = InventoryTarget::Player(5);
    ledger
        .begin_target_gesture(destination, CellGesture::Click)
        .unwrap();
    assert!(ledger.target_retains_stack_id(destination, 9));
    assert!(!ledger.target_retains_stack_id(source, 9));

    // A replacement of the same kind retains its own identity.
    let mut replacement = ledger_with_slot_zero().displayed_stack(0).unwrap().clone();
    replacement.stack_network_id = 99;
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: protocol::ContainerIdentity::window(0),
            slot: 6,
        },
        stack: replacement,
        storage_item: None,
    }));
    let other = InventoryTarget::Player(6);
    ledger.begin_hotbar_swap(destination, 6).unwrap();
    assert!(!ledger.target_retains_stack_id(destination, 9));
    assert!(ledger.target_retains_stack_id(destination, 99));
    assert!(ledger.target_retains_stack_id(other, 9));
}

#[test]
fn a_fresh_slot_write_discards_the_old_corrected_identity() {
    use super::InventoryTarget;
    let mut ledger = ledger_with_slot_zero();
    let target = InventoryTarget::Player(0);
    let request = ledger.begin_mining_request(0, 4).unwrap();
    ledger.apply(&mining_response(
        request,
        StackResponseStatus::Accepted,
        4,
        77,
    ));
    assert!(ledger.target_retains_stack_id(target, 9));
    let stack = ledger.displayed_stack(0).unwrap().clone();
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: protocol::ContainerIdentity::window(0),
            slot: 0,
        },
        stack,
        storage_item: None,
    }));
    assert!(!ledger.target_retains_stack_id(target, 9));
    assert!(ledger.target_retains_stack_id(target, 77));
}

#[test]
fn splitting_a_stack_retains_the_source_identity_only_on_the_remainder() {
    use super::InventoryTarget;
    let mut ledger = ledger_with_slot_zero();
    acknowledge_personal_open(&mut ledger, 2);
    let corrected_request = ledger.begin_world_drop(0, Some(1)).unwrap();
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            request_id: corrected_request,
            status: StackResponseStatus::Accepted,
            containers: Arc::from([correction(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                31,
                77,
            )]),
        }]),
    }));
    ledger.begin_take_count(0, 1).unwrap();
    ledger.begin_click(5).unwrap();
    assert!(ledger.target_retains_stack_id(InventoryTarget::Player(0), 9));
    assert!(!ledger.target_retains_stack_id(InventoryTarget::Player(5), 9));
    assert!(!ledger.target_retains_stack_id(InventoryTarget::Player(5), corrected_request));
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 30);
    assert_eq!(ledger.displayed_stack(5).unwrap().count, 1);
}
