use protocol::test_support::transactions::observe_transaction;
use protocol::{
    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, CONTAINER_NAME_CURSOR, ItemStackResponseEvent,
    StackResponse, StackResponseContainer, StackResponseSlot, StackResponseStatus,
};

use super::*;

mod history_tests;
mod lifecycle;
mod retirement;

/// One addressed count and stack-id correction, with its requested cell unchanged.
fn corrected_container(
    slot_type: u8,
    slot: u8,
    count: u8,
    item_stack_id: i32,
) -> StackResponseContainer {
    StackResponseContainer {
        container: ContainerIdentity {
            window_id: None,
            slot_type: Some(slot_type),
            dynamic_id: None,
        },
        slots: Arc::from([StackResponseSlot {
            slot,
            hotbar_slot: slot,
            count,
            item_stack_id,
            custom_name: Arc::from("corrected"),
            filtered_custom_name: Arc::from(""),
            durability_correction: -1,
        }]),
    }
}

/// A response to the drop in slot 2, with an optional authoritative count and stack id.
fn response(id: i32, status: StackResponseStatus, correction: Option<(u8, i32)>) -> InventoryEvent {
    let containers = correction
        .map(|(count, item_stack_id)| {
            corrected_container(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                2,
                count,
                item_stack_id,
            )
        })
        .into_iter()
        .collect::<Vec<_>>();
    InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status,
            request_id: id,
            containers: containers.into(),
        }]),
    })
}

/// Opens the personal inventory without refreshing its retained player stacks.
fn open_personal(ledger: &mut PlayerInventoryLedger) {
    assert!(ledger.request_personal_open(42));
    assert!(ledger.mark_transport_enqueued(10));
    ledger.apply(&InventoryEvent::Open(ContainerOpenEvent {
        container: ContainerIdentity::window(2),
        window_type: PERSONAL_INVENTORY_WINDOW_TYPE,
        position: [0, 64, 0],
        runtime_entity_id: -1,
    }));
}

#[test]
fn legacy_drop_responses_reconcile_positive_and_legacy_stack_ids_once() {
    for corrected_id in [91, -2] {
        let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
        let request = ledger.begin_world_drop(2, Some(1)).unwrap();
        let request_legacy = drop_legacy_id(&ledger, request);
        assert!(ledger.mark_transport_enqueued(0));
        assert_eq!(ledger.pending_request_count(), 0);
        ledger.apply(&response(
            request_legacy,
            StackResponseStatus::Accepted,
            Some((3, corrected_id)),
        ));
        let held = ledger.displayed_stack(2).unwrap();
        assert_eq!((held.count, held.stack_network_id), (3, corrected_id));
        assert_eq!(
            ledger.slot_overlay(2).unwrap().custom_name.as_deref(),
            Some("corrected")
        );
        assert!(!ledger.resync_required());

        ledger.apply(&response(
            request_legacy,
            StackResponseStatus::Rejected,
            None,
        ));
        assert_eq!(ledger.displayed_stack(2).unwrap().count, 3);
    }
}

#[test]
fn rejected_legacy_drop_restores_the_original_stack_and_overlay_once() {
    let original = digested(stack(6, 100, 5));
    let mut ledger = known_ledger(&[(2, original.clone())]);
    let overlay = StackResponseOverlay {
        custom_name: Some(Arc::from("original")),
        ..Default::default()
    };
    ledger.set_confirmed_overlay(Cell::Inventory(2), overlay.clone());
    let original = ledger.displayed_stack(2).unwrap().clone();
    let request = ledger.begin_world_drop(2, Some(1)).unwrap();
    let request_legacy = drop_legacy_id(&ledger, request);
    assert_eq!(ledger.take_world_drops(), 1);
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&response(
        request_legacy,
        StackResponseStatus::Rejected,
        None,
    ));
    assert_eq!(ledger.displayed_stack(2), Some(&original));
    assert_eq!(ledger.slot_overlay(2), Some(&overlay));
    assert_eq!(ledger.pending_request_count(), 0);
    assert_eq!(ledger.take_world_drops(), 0);

    ledger.apply(&response(
        request_legacy,
        StackResponseStatus::Accepted,
        Some((1, 91)),
    ));
    assert_eq!(ledger.displayed_stack(2), Some(&original));
}

#[test]
fn unanswered_legacy_drops_do_not_delay_play_or_require_a_refresh() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    let request = ledger.begin_world_drop(2, Some(1)).unwrap();
    let request_legacy = drop_legacy_id(&ledger, request);
    assert!(ledger.mark_transport_enqueued(0));
    ledger.poll_timeout(INVENTORY_REQUEST_TIMEOUT_MILLIS * 2);
    assert_eq!(ledger.pending_state(), None);
    assert!(!ledger.resync_required());
    assert!(ledger.pending_batch().unwrap().is_none());
    ledger.apply(&response(
        request_legacy,
        StackResponseStatus::Accepted,
        Some((3, 91)),
    ));
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 3);
}

#[test]
fn queued_drops_serialize_the_established_legacy_source_and_world_descriptor() {
    for second_amount in [1, 2] {
        let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 3)))]);
        let first = ledger.begin_world_drop(2, Some(1)).unwrap();
        let first_legacy = drop_legacy_id(&ledger, first);
        let second = ledger.begin_world_drop(2, Some(second_amount)).unwrap();
        let second_legacy = drop_legacy_id(&ledger, second);
        assert!(ledger.mark_transport_enqueued(0));
        let (packet, count) = ledger.pending_batch().unwrap().unwrap();
        assert_eq!(count, 1);
        let transaction = observe_transaction(&packet).unwrap();
        assert_eq!(transaction.actions.len(), 2);
        assert_eq!(transaction.actions[0].from_item.stack_network_id, None);
        assert_eq!(transaction.actions[0].from_item.count, 2);
        assert_eq!(transaction.actions[1].to_item.stack_network_id, None);
        assert_eq!(transaction.actions[1].to_item.count, second_amount);
        assert_eq!(
            transaction.legacy_request_id,
            if second_amount == 1 { second_legacy } else { 0 }
        );
        assert!(ledger.mark_transport_enqueued(0));
        assert_eq!(
            ledger.displayed_stack(2).map(|stack| stack.count),
            if second_amount == 1 { Some(1) } else { None }
        );
    }
}

#[test]
fn legacy_remainders_merge_on_either_side_of_a_cursor_gesture() {
    for take in [false, true] {
        let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
        let request = ledger.begin_world_drop(2, Some(1)).unwrap();
        let request_legacy = drop_legacy_id(&ledger, request);
        assert!(ledger.mark_transport_enqueued(0));
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity {
                window_id: None,
                slot_type: Some(CONTAINER_NAME_CURSOR),
                dynamic_id: None,
            },
            slots: Arc::from([digested(stack(6, 101, 2))]),
            storage_item: NetworkItemStack::empty(),
        }));
        open_personal(&mut ledger);
        if take {
            ledger.begin_take_count(2, 1).unwrap();
            assert_eq!(ledger.displayed_stack(2).unwrap().count, 3);
            assert_eq!(ledger.cursor_stack().unwrap().count, 3);
            assert!(
                matches!(ledger.newest_action(), Some(StackRequestAction::Take { source, .. }) if source.stack_network_id == request_legacy)
            );
        } else {
            ledger.begin_place_count(2, 1).unwrap();
            assert_eq!(ledger.displayed_stack(2).unwrap().count, 5);
            assert_eq!(ledger.cursor_stack().unwrap().count, 1);
            assert!(
                matches!(ledger.newest_action(), Some(StackRequestAction::Place { destination, .. }) if destination.stack_network_id == request_legacy)
            );
        }
        assert!(ledger.pending_batch().unwrap().is_some());
    }
}

#[test]
fn two_stacks_with_the_same_settled_legacy_id_cannot_merge() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    let request = ledger.begin_world_drop(2, Some(1)).unwrap();
    let request_legacy = drop_legacy_id(&ledger, request);
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
        container: ContainerIdentity {
            window_id: None,
            slot_type: Some(CONTAINER_NAME_CURSOR),
            dynamic_id: None,
        },
        slots: Arc::from([digested(stack(6, request_legacy, 2))]),
        storage_item: NetworkItemStack::empty(),
    }));
    open_personal(&mut ledger);
    assert_eq!(
        ledger.begin_place_count(2, 1),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 4);
    assert_eq!(ledger.cursor_stack().unwrap().count, 2);
}

#[test]
fn quick_moves_merge_legacy_remainders_as_sources_and_destinations() {
    for source in [2, 20] {
        let mut ledger = known_ledger(&[
            (2, digested(stack(6, 100, 5))),
            (20, digested(stack(6, 101, 2))),
        ]);
        let request = ledger.begin_world_drop(2, Some(1)).unwrap();
        let request_legacy = drop_legacy_id(&ledger, request);
        assert!(ledger.mark_transport_enqueued(0));
        open_personal(&mut ledger);
        ledger
            .begin_quick_move(InventoryTarget::Player(source))
            .unwrap();
        let destination = if source == 2 { 20 } else { 2 };
        assert_eq!(ledger.displayed_stack(destination).unwrap().count, 6);
        assert!(ledger.displayed_stack(source).is_none());
        let Some(StackRequestAction::Place {
            source: from,
            destination: into,
            ..
        }) = ledger.newest_action()
        else {
            panic!("a merge place");
        };
        assert_eq!((from.slot, into.slot), (source, destination));
        let legacy_id = if source == 2 {
            from.stack_network_id
        } else {
            into.stack_network_id
        };
        assert_eq!(legacy_id, request_legacy);
    }
}

#[test]
fn split_responses_skip_negative_screen_ids_and_keep_positive_siblings() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    let drop = ledger.begin_world_drop(2, Some(1)).unwrap();
    let drop_legacy = drop_legacy_id(&ledger, drop);
    assert!(ledger.mark_transport_enqueued(0));
    open_personal(&mut ledger);
    let split = ledger.begin_take_count(2, 2).unwrap();
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id: split,
            containers: Arc::from([
                corrected_container(
                    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                    2,
                    2,
                    drop_legacy,
                ),
                corrected_container(CONTAINER_NAME_CURSOR, 0, 2, 91),
            ]),
        }]),
    }));
    assert_eq!(
        ledger.displayed_stack(2).unwrap().stack_network_id,
        drop_legacy
    );
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 4);
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 91);
    assert_eq!(ledger.pending_state(), None);
    assert!(!ledger.resync_required());
    ledger.apply(&response(
        drop_legacy,
        StackResponseStatus::Accepted,
        Some((4, 92)),
    ));
    ledger.apply(&response(drop_legacy, StackResponseStatus::Rejected, None));
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 4);
    assert_eq!(ledger.displayed_stack(2).unwrap().stack_network_id, 92);
}

#[test]
fn a_later_rejected_drop_reveals_the_corrected_earlier_drop() {
    for status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
        let original = digested(stack(6, 100, 5));
        let mut ledger = known_ledger(&[(2, original.clone())]);
        let first = ledger.begin_world_drop(2, Some(1)).unwrap();
        let first_legacy = drop_legacy_id(&ledger, first);
        let second = ledger.begin_world_drop(2, Some(1)).unwrap();
        let second_legacy = drop_legacy_id(&ledger, second);
        assert!(ledger.mark_transport_enqueued(0));
        assert!(ledger.mark_transport_enqueued(0));
        ledger.apply(&response(
            first_legacy,
            status,
            (status == StackResponseStatus::Accepted).then_some((2, 91)),
        ));
        assert_eq!(ledger.displayed_stack(2).unwrap().count, 3);
        ledger.apply(&response(
            second_legacy,
            StackResponseStatus::Rejected,
            None,
        ));
        if status == StackResponseStatus::Accepted {
            let held = ledger.displayed_stack(2).unwrap();
            assert_eq!((held.count, held.stack_network_id), (2, 91));
            assert_eq!(
                ledger.slot_overlay(2).unwrap().custom_name.as_deref(),
                Some("corrected")
            );
        } else {
            assert_eq!(ledger.displayed_stack(2), Some(&original));
        }
    }
}

#[test]
fn out_of_order_rejections_restore_the_original_stack() {
    let original = digested(stack(6, 100, 5));
    let mut ledger = known_ledger(&[(2, original.clone())]);
    let first = ledger.begin_world_drop(2, Some(1)).unwrap();
    let first_legacy = drop_legacy_id(&ledger, first);
    let second = ledger.begin_world_drop(2, Some(1)).unwrap();
    let second_legacy = drop_legacy_id(&ledger, second);
    assert!(ledger.mark_transport_enqueued(0));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&response(
        second_legacy,
        StackResponseStatus::Rejected,
        None,
    ));
    assert_eq!(
        ledger.displayed_stack(2).unwrap().stack_network_id,
        first_legacy
    );
    ledger.apply(&response(first_legacy, StackResponseStatus::Rejected, None));
    assert_eq!(ledger.displayed_stack(2), Some(&original));
}

#[test]
fn late_drop_responses_do_not_restore_an_authoritatively_emptied_slot() {
    for status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
        let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 2)))]);
        let request = ledger.begin_world_drop(2, Some(1)).unwrap();
        let request_legacy = drop_legacy_id(&ledger, request);
        assert!(ledger.mark_transport_enqueued(0));
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 2,
            },
            stack: NetworkItemStack::empty(),
            storage_item: None,
        }));
        ledger.apply(&response(
            request_legacy,
            status,
            (status == StackResponseStatus::Accepted).then_some((1, 91)),
        ));
        assert_eq!(ledger.slot_state(2), Some(PlayerInventorySlot::Empty));
    }
}

#[test]
fn later_sparse_predictions_still_cover_a_legacy_response_to_the_backing_slot() {
    for status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
        let original = digested(stack(6, 100, 5));
        let mut ledger = known_ledger(&[(2, original.clone())]);
        let first = ledger.begin_world_drop(2, Some(1)).unwrap();
        let first_legacy = drop_legacy_id(&ledger, first);
        let second = ledger.begin_world_drop(2, Some(1)).unwrap();
        assert!(ledger.mark_transport_enqueued(0));
        ledger.apply(&response(
            first_legacy,
            status,
            (status == StackResponseStatus::Accepted).then_some((2, 91)),
        ));
        assert_eq!(
            ledger.displayed_stack(2).unwrap().count,
            if status == StackResponseStatus::Accepted {
                1
            } else {
                4
            }
        );
        assert_eq!(ledger.displayed_stack(2).unwrap().stack_network_id, second);
        assert_eq!(
            ledger.confirmed_stack(Cell::Inventory(2)).unwrap().count,
            if status == StackResponseStatus::Accepted {
                2
            } else {
                5
            }
        );
    }
}

#[test]
fn late_legacy_responses_do_not_overwrite_a_server_restatement_or_a_newer_drop() {
    for status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
        for refreshed in [false, true] {
            let original = digested(stack(6, 100, 5));
            let mut ledger = known_ledger(&[(2, original.clone())]);
            let first = ledger.begin_world_drop(2, Some(1)).unwrap();
            let first_legacy = drop_legacy_id(&ledger, first);
            assert!(ledger.mark_transport_enqueued(0));
            if refreshed {
                ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity::window(0),
                        slot: 2,
                    },
                    stack: original,
                    storage_item: None,
                }));
            } else {
                ledger.begin_world_drop(2, Some(1)).unwrap();
                assert!(ledger.mark_transport_enqueued(0));
            }
            let current = ledger.displayed_stack(2).cloned();
            ledger.apply(&response(
                first_legacy,
                status,
                (status == StackResponseStatus::Accepted).then_some((1, 91)),
            ));
            assert_eq!(ledger.displayed_stack(2), current.as_ref());
        }
    }
}
