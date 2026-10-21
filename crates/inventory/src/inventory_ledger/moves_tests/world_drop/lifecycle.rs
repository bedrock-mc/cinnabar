use protocol::test_support::transactions::TransactionObservation;

use super::*;

/// An admitted empty drop can roll back until a server restatement invalidates its snapshot.
#[test]
fn rejecting_an_empty_world_drop_restores_only_its_current_backing() {
    for refreshed in [false, true] {
        let original = digested(stack(6, 100, 3));
        let mut ledger = known_ledger(&[(2, original.clone())]);
        let drop = ledger.begin_world_drop(2, None).unwrap();
        let legacy = drop_legacy_id(&ledger, drop);
        assert!(ledger.mark_transport_enqueued(0));
        assert!(ledger.displayed_stack(2).is_none());
        if refreshed {
            ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot: 2,
                },
                stack: NetworkItemStack::empty(),
                storage_item: None,
            }));
        }
        ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
        assert_eq!(ledger.displayed_stack(2), (!refreshed).then_some(&original));
        assert_eq!(ledger.pending_request_count(), 0);
        assert!(!ledger.resync_required());
    }
}

/// Observes the next drop's exact descriptors without admitting its transport.
fn queued_transaction(ledger: &PlayerInventoryLedger) -> TransactionObservation {
    let (packet, count) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(count, 1);
    observe_transaction(&packet).unwrap()
}

/// Publishes one already-admitted gameplay decrement against the current backing slot.
fn consume(ledger: &mut PlayerInventoryLedger, slot: u8) -> protocol::PredictedSlotChange {
    let stack = ledger
        .confirmed_stack(Cell::Inventory(slot))
        .unwrap()
        .clone();
    let from =
        protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
    let legacy_request_id = protocol::next_legacy_request_id().unwrap();
    let change = protocol::PredictedSlotChange {
        to: from.less_one(legacy_request_id),
        from,
        legacy_request_id,
    };
    let revision = ledger.authoritative_slot_revision(slot).unwrap();
    assert!(ledger.settle_use_slot_change(slot, revision, &change));
    change
}

#[test]
fn accepting_a_sent_drop_rebuilds_the_next_unsent_drop() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    let first = ledger.begin_world_drop(2, Some(1)).unwrap();
    let first_legacy = drop_legacy_id(&ledger, first);
    let second = ledger.begin_world_drop(2, Some(1)).unwrap();
    let second_legacy = drop_legacy_id(&ledger, second);
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&response(
        first_legacy,
        StackResponseStatus::Accepted,
        Some((3, 91)),
    ));
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 2);
    let transaction = queued_transaction(&ledger);
    assert_eq!(transaction.actions[0].from_item.count, 3);
    assert_eq!(
        protocol::item_custom_name(&transaction.actions[0].from_item.user_data).as_deref(),
        Some("corrected")
    );
    assert_eq!(
        protocol::item_extra_damage_value(&transaction.actions[0].from_item.user_data),
        Some(-1)
    );
    assert_eq!(transaction.actions[0].from_item.stack_network_id, Some(91));
    assert_eq!(transaction.actions[0].to_item.count, 2);
    assert_eq!(transaction.actions[1].to_item.stack_network_id, Some(91));
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&response(
        second_legacy,
        StackResponseStatus::Rejected,
        None,
    ));
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 3);
    assert_eq!(ledger.displayed_stack(2).unwrap().stack_network_id, 91);
}

#[test]
fn rejecting_two_drops_across_transport_backpressure_restores_every_item() {
    let original = digested(stack(6, 100, 5));
    let mut ledger = known_ledger(&[(2, original.clone())]);
    let first = ledger.begin_world_drop(2, Some(1)).unwrap();
    let first_legacy = drop_legacy_id(&ledger, first);
    let second = ledger.begin_world_drop(2, Some(1)).unwrap();
    let second_legacy = drop_legacy_id(&ledger, second);
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&response(first_legacy, StackResponseStatus::Rejected, None));
    let transaction = queued_transaction(&ledger);
    assert_eq!(transaction.actions[0].from_item.count, 5);
    assert_eq!(transaction.actions[0].from_item.stack_network_id, Some(100));
    assert_eq!(transaction.actions[0].to_item.count, 4);
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&response(
        second_legacy,
        StackResponseStatus::Rejected,
        None,
    ));
    assert_eq!(ledger.displayed_stack(2), Some(&original));
}

#[test]
fn a_source_slot_or_content_restatement_retires_unsent_drops() {
    for content in [false, true] {
        for sent_first in [false, true] {
            let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
            let first = ledger.begin_world_drop(2, Some(1)).unwrap();
            let first_legacy = drop_legacy_id(&ledger, first);
            if sent_first {
                assert!(ledger.mark_transport_enqueued(0));
            }
            ledger.begin_world_drop(2, Some(1)).unwrap();
            let replacement = digested(stack(8, 91, 7));
            if content {
                let mut slots = vec![NetworkItemStack::empty(); PLAYER_INVENTORY_SLOT_COUNT];
                slots[2] = replacement.clone();
                ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                    container: ContainerIdentity::window(0),
                    slots: slots.into(),
                    storage_item: NetworkItemStack::empty(),
                }));
            } else {
                ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                    identity: SlotIdentity {
                        container: ContainerIdentity::window(0),
                        slot: 2,
                    },
                    stack: replacement.clone(),
                    storage_item: None,
                }));
            }
            assert_eq!(ledger.pending_request_count(), 0);
            assert!(ledger.pending_batch().unwrap().is_none());
            assert_eq!(ledger.displayed_stack(2), Some(&replacement));
            ledger.apply(&response(first_legacy, StackResponseStatus::Rejected, None));
            assert_eq!(ledger.displayed_stack(2), Some(&replacement));
        }
    }
}

#[test]
fn restating_an_unrelated_slot_keeps_the_drop_queued() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    ledger.begin_world_drop(2, Some(1)).unwrap();
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 3,
        },
        stack: digested(stack(8, 91, 7)),
        storage_item: None,
    }));
    assert_eq!(queued_transaction(&ledger).actions[0].from_item.count, 5);
}

#[test]
fn a_screen_request_correction_rebuilds_a_drop_queued_on_its_source() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    open_personal(&mut ledger);
    let screen = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
        .unwrap();
    assert!(ledger.mark_transport_enqueued(0));
    ledger.begin_world_drop(2, Some(1)).unwrap();
    ledger.apply(&response(
        screen,
        StackResponseStatus::Accepted,
        Some((4, 91)),
    ));
    let transaction = queued_transaction(&ledger);
    assert_eq!(transaction.actions[0].from_item.count, 4);
    assert_eq!(transaction.actions[0].from_item.stack_network_id, Some(91));
    assert_eq!(transaction.actions[0].to_item.count, 3);
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 3);
}

#[test]
fn admitted_consumption_is_in_the_next_drops_source_and_rollback() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 3)))]);
    let change = consume(&mut ledger, 2);
    let request = ledger.begin_world_drop(2, Some(1)).unwrap();
    let legacy = drop_legacy_id(&ledger, request);
    let transaction = queued_transaction(&ledger);
    assert_eq!(transaction.actions[0].from_item.count, 2);
    assert_eq!(
        transaction.actions[0].from_item.stack_network_id,
        Some(change.legacy_request_id)
    );
    assert_eq!(transaction.actions[0].to_item.count, 1);
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 2);
    assert_eq!(
        ledger.displayed_stack(2).unwrap().stack_network_id,
        change.legacy_request_id
    );
}

#[test]
fn admitted_consumption_rebuilds_an_already_queued_drop() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 3)))]);
    let request = ledger.begin_world_drop(2, Some(1)).unwrap();
    let legacy = drop_legacy_id(&ledger, request);
    consume(&mut ledger, 2);
    let transaction = queued_transaction(&ledger);
    assert_eq!(transaction.actions[0].from_item.count, 2);
    assert_eq!(transaction.actions[0].to_item.count, 1);
    assert!(ledger.mark_transport_enqueued(0));
    ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 2);
}

#[test]
fn consumption_between_drops_survives_each_acceptance_or_rejection() {
    for status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
        let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
        let first = ledger.begin_world_drop(2, Some(1)).unwrap();
        let first_legacy = drop_legacy_id(&ledger, first);
        let second = ledger.begin_world_drop(2, Some(1)).unwrap();
        let second_legacy = drop_legacy_id(&ledger, second);
        assert!(ledger.mark_transport_enqueued(0));
        consume(&mut ledger, 2);
        ledger.apply(&response(
            first_legacy,
            status,
            (status == StackResponseStatus::Accepted).then_some((4, 91)),
        ));
        let count = if status == StackResponseStatus::Accepted {
            3
        } else {
            4
        };
        let transaction = queued_transaction(&ledger);
        assert_eq!(transaction.actions[0].from_item.count, count);
        assert_eq!(transaction.actions[0].to_item.count, count - 1);
        assert!(ledger.mark_transport_enqueued(0));
        ledger.apply(&response(
            second_legacy,
            StackResponseStatus::Rejected,
            None,
        ));
        assert_eq!(ledger.displayed_stack(2).unwrap().count, count);
    }
}

#[test]
fn consuming_the_last_remainder_still_allows_the_drop_to_reconcile() {
    for status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
        let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 2)))]);
        let request = ledger.begin_world_drop(2, Some(1)).unwrap();
        let legacy = drop_legacy_id(&ledger, request);
        assert!(ledger.mark_transport_enqueued(0));
        consume(&mut ledger, 2);
        assert_eq!(ledger.slot_state(2), Some(PlayerInventorySlot::Empty));
        ledger.apply(&response(
            legacy,
            status,
            (status == StackResponseStatus::Accepted).then_some((1, 91)),
        ));
        assert_eq!(
            ledger.displayed_stack(2).map(|stack| stack.count),
            if status == StackResponseStatus::Accepted {
                None
            } else {
                Some(1)
            }
        );
    }
}

#[test]
fn consumption_that_empties_an_unsent_drops_source_cancels_the_drop() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 1)))]);
    ledger.begin_world_drop(2, Some(1)).unwrap();
    consume(&mut ledger, 2);
    assert!(ledger.pending_batch().unwrap().is_none());
    assert_eq!(ledger.slot_state(2), Some(PlayerInventorySlot::Empty));
}

#[test]
fn a_stale_gameplay_source_cannot_replace_a_server_restatement() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 3)))]);
    let stack = ledger.displayed_stack(2).unwrap().clone();
    let from =
        protocol::VerifiedNetworkItemStack::try_new(stack.clone(), stack.nbt_digest).unwrap();
    let legacy_request_id = protocol::next_legacy_request_id().unwrap();
    let change = protocol::PredictedSlotChange {
        to: from.less_one(legacy_request_id),
        from,
        legacy_request_id,
    };
    let revision = ledger.authoritative_slot_revision(2).unwrap();
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 2,
        },
        stack: digested(super::stack(8, 91, 7)),
        storage_item: None,
    }));
    assert!(!ledger.settle_use_slot_change(2, revision, &change));
    assert_eq!(ledger.displayed_stack(2).unwrap().count, 7);
    assert_eq!(ledger.displayed_stack(2).unwrap().stack_network_id, 91);
}
