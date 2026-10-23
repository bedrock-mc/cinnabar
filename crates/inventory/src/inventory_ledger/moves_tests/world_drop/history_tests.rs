use super::*;

/// Rolling back a normal drop can restore an earlier screen request's empty backing owner.
#[test]
fn rejecting_take_place_and_empty_world_drop_restores_the_entire_stack() {
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0]] {
        let original = digested(stack(6, 100, 3));
        let mut ledger = known_ledger(&[(2, original.clone())]);
        open_personal(&mut ledger);
        let take = ledger.begin_click(2).unwrap();
        let place = ledger.begin_click(2).unwrap();
        let world = ledger.begin_world_drop(2, None).unwrap();
        let legacy = drop_legacy_id(&ledger, world);
        while ledger.pending_batch().unwrap().is_some() {
            assert!(ledger.mark_transport_enqueued(20));
        }
        let ids = [take, place, legacy];
        for index in order {
            ledger.apply(&response(ids[index], StackResponseStatus::Rejected, None));
        }
        assert_eq!(ledger.displayed_stack(2), Some(&original));
        assert!(ledger.cursor_stack().is_none());
        assert_eq!(ledger.pending_request_count(), 0);
        assert!(!ledger.resync_required());
    }
}

/// Sends a screen drop and local close while the acknowledgement remains outstanding.
fn screen_drop_and_close(ledger: &mut PlayerInventoryLedger) -> i32 {
    open_personal(ledger);
    let screen = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
        .unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.request_personal_close();
    assert!(ledger.mark_transport_enqueued(21));
    assert!(!ledger.personal_inventory_desired_open());
    screen
}

#[test]
fn screen_drops_and_swaps_reconcile_with_world_drops_in_both_reply_orders() {
    for swap in [false, true] {
        for predecessor_status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
            for drop_status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
                for predecessor_first in [false, true] {
                    let original = digested(stack(6, 100, if swap { 3 } else { 5 }));
                    let mut ledger =
                        known_ledger(&[(2, original.clone()), (20, digested(stack(8, 200, 5)))]);
                    let predecessor = if swap {
                        open_personal(&mut ledger);
                        let request = ledger
                            .begin_hotbar_swap(InventoryTarget::Player(20), 2)
                            .unwrap();
                        assert!(ledger.mark_transport_enqueued(20));
                        request
                    } else {
                        screen_drop_and_close(&mut ledger)
                    };
                    let world = ledger.begin_world_drop(2, Some(1)).unwrap();
                    let legacy = drop_legacy_id(&ledger, world);
                    assert!(ledger.mark_transport_enqueued(22));
                    let dropped_count = if swap { 4 } else { 3 };
                    assert_eq!(ledger.displayed_stack(2).unwrap().count, dropped_count);
                    let corrected_count = if swap { 5 } else { 3 };
                    let predecessor_accepted = predecessor_status == StackResponseStatus::Accepted;
                    let drop_accepted = drop_status == StackResponseStatus::Accepted;
                    let item = if swap { 8 } else { 6 };
                    let predecessor_reply = response(
                        predecessor,
                        predecessor_status,
                        predecessor_accepted.then_some((corrected_count, 91)),
                    );
                    let drop_reply = response(
                        legacy,
                        drop_status,
                        drop_accepted.then_some((dropped_count as u8, 93)),
                    );
                    let replies = if predecessor_first {
                        [predecessor_reply, drop_reply]
                    } else {
                        [drop_reply, predecessor_reply]
                    };
                    for reply in replies {
                        ledger.apply(&reply);
                    }
                    let expected = if drop_accepted {
                        (item, dropped_count, 93)
                    } else if predecessor_accepted {
                        (item, u16::from(corrected_count), 91)
                    } else {
                        (
                            original.network_id,
                            original.count,
                            original.stack_network_id,
                        )
                    };
                    let held = ledger.displayed_stack(2).unwrap();
                    assert_eq!(
                        (held.network_id, held.count, held.stack_network_id),
                        expected
                    );
                    if !predecessor_accepted && !drop_accepted {
                        assert_eq!(ledger.displayed_stack(2), Some(&original));
                    }
                    assert_eq!(ledger.pending_request_count(), 0);
                    assert!(!ledger.resync_required());
                    assert!(ledger.begin_world_drop(2, Some(1)).is_ok());
                }
            }
        }
    }
}

#[test]
fn rejecting_a_screen_predecessor_rebuilds_an_unsent_world_drop() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    let screen = screen_drop_and_close(&mut ledger);
    let world = ledger.begin_world_drop(2, Some(1)).unwrap();
    let legacy = drop_legacy_id(&ledger, world);
    ledger.apply(&response(screen, StackResponseStatus::Rejected, None));
    let (packet, count) = ledger.pending_batch().unwrap().unwrap();
    assert_eq!(count, 1);
    let transaction = observe_transaction(&packet).unwrap();
    assert_eq!(transaction.actions[0].from_item.count, 5);
    assert_eq!(transaction.actions[0].from_item.stack_network_id, None);
    assert_eq!(transaction.actions[0].to_item.count, 4);
    assert!(ledger.mark_transport_enqueued(22));
    ledger.apply(&response(legacy, StackResponseStatus::Rejected, None));
    let held = ledger.displayed_stack(2).unwrap();
    assert_eq!((held.count, held.stack_network_id), (5, 100));
}

#[test]
fn world_replies_reconcile_sent_and_unsent_screen_successors() {
    for sent in [false, true] {
        for status in [StackResponseStatus::Accepted, StackResponseStatus::Rejected] {
            for screen_first in [false, true] {
                if !sent && screen_first {
                    continue;
                }
                let original = digested(stack(6, 100, 5));
                let mut ledger = known_ledger(&[(2, original.clone())]);
                let world = ledger.begin_world_drop(2, Some(1)).unwrap();
                let legacy = drop_legacy_id(&ledger, world);
                assert!(ledger.mark_transport_enqueued(0));
                open_personal(&mut ledger);
                let screen = ledger
                    .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
                    .unwrap();
                if sent {
                    assert!(ledger.mark_transport_enqueued(20));
                }
                if screen_first {
                    ledger.apply(&response(screen, StackResponseStatus::Rejected, None));
                }
                ledger.apply(&response(
                    legacy,
                    status,
                    (status == StackResponseStatus::Accepted).then_some((3, 91)),
                ));
                let (count, name) = if status == StackResponseStatus::Accepted {
                    (3, 91)
                } else {
                    (5, 100)
                };
                if !sent {
                    assert_eq!(ledger.displayed_stack(2).unwrap().count, count - 1);
                    assert!(ledger.pending_batch().unwrap().is_some());
                    assert!(ledger.mark_transport_enqueued(20));
                }
                if !screen_first {
                    ledger.apply(&response(screen, StackResponseStatus::Rejected, None));
                }
                let held = ledger.displayed_stack(2).unwrap();
                assert_eq!((held.count, held.stack_network_id), (count, name));
                if status == StackResponseStatus::Rejected {
                    assert_eq!(ledger.displayed_stack(2), Some(&original));
                }
                assert!(!ledger.resync_required());
                assert!(
                    ledger
                        .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
                        .is_ok()
                );
            }
        }
    }
}

#[test]
fn an_empty_world_correction_cancels_its_unsent_successor_without_reviving_older_predictions() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
    let screen = screen_drop_and_close(&mut ledger);
    let world = ledger.begin_world_drop(2, Some(1)).unwrap();
    let legacy = drop_legacy_id(&ledger, world);
    assert!(ledger.mark_transport_enqueued(22));
    ledger.begin_world_drop(2, Some(1)).unwrap();
    ledger.apply(&response(
        legacy,
        StackResponseStatus::Accepted,
        Some((0, 0)),
    ));
    assert!(ledger.pending_batch().unwrap().is_none());
    assert_eq!(ledger.slot_state(2), Some(PlayerInventorySlot::Empty));
    ledger.apply(&response(screen, StackResponseStatus::Rejected, None));
    assert_eq!(ledger.slot_state(2), Some(PlayerInventorySlot::Empty));
    assert!(!ledger.resync_required());
}

#[test]
fn interleaved_screen_and_world_rejections_restore_the_entire_chain() {
    for order in [[0, 1, 2, 3], [3, 2, 1, 0], [1, 2, 0, 3]] {
        let original = digested(stack(6, 100, 7));
        let mut ledger = known_ledger(&[(2, original.clone())]);
        open_personal(&mut ledger);
        let mut ids = Vec::new();
        for world in [false, true, false, true] {
            if world {
                let request = ledger.begin_world_drop(2, Some(1)).unwrap();
                ids.push(drop_legacy_id(&ledger, request));
            } else {
                ids.push(
                    ledger
                        .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
                        .unwrap(),
                );
            }
            assert!(ledger.mark_transport_enqueued(20));
        }
        for index in order {
            ledger.apply(&response(ids[index], StackResponseStatus::Rejected, None));
        }
        assert_eq!(ledger.displayed_stack(2), Some(&original));
        assert_eq!(ledger.pending_request_count(), 0);
        assert!(!ledger.resync_required());
    }
}

#[test]
fn an_empty_admitted_drop_supersedes_a_delayed_mining_correction() {
    let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 1)))]);
    let mining = ledger.begin_mining_request(2, 1).unwrap();
    ledger.begin_world_drop(2, None).unwrap();
    assert!(ledger.mark_transport_enqueued(20));
    ledger.apply(&response(
        mining,
        StackResponseStatus::Accepted,
        Some((1, 91)),
    ));
    assert_eq!(ledger.slot_state(2), Some(PlayerInventorySlot::Empty));
    assert_eq!(ledger.pending_state(), None);
    assert!(!ledger.resync_required());
    assert!(ledger.begin_world_drop(2, None).is_err());
}

#[test]
fn rejecting_screen_history_preserves_a_newer_slot_write() {
    for reverse in [false, true] {
        for replacement in [NetworkItemStack::empty(), digested(stack(8, 91, 7))] {
            let mut ledger = known_ledger(&[(2, digested(stack(6, 100, 5)))]);
            open_personal(&mut ledger);
            let mut ids: [i32; 2] = std::array::from_fn(|_| {
                let id = ledger
                    .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
                    .unwrap();
                assert!(ledger.mark_transport_enqueued(20));
                id
            });
            ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
                identity: SlotIdentity {
                    container: ContainerIdentity::window(0),
                    slot: 2,
                },
                stack: replacement.clone(),
                storage_item: None,
            }));
            if reverse {
                ids.reverse();
            }
            for id in ids {
                ledger.apply(&response(id, StackResponseStatus::Rejected, None));
            }
            assert_eq!(
                ledger.displayed_stack(2),
                (!replacement.is_empty()).then_some(&replacement)
            );
            assert_eq!(ledger.pending_state(), None);
            assert!(!ledger.resync_required());
        }
    }
}

#[test]
fn rejecting_a_swap_cancels_unsent_drops_instead_of_retargeting_the_restored_item() {
    for (network_id, metadata) in [(8, 0), (6, 1)] {
        let original = digested(stack(6, 100, 5));
        let mut swapped = stack(network_id, 200, 5);
        swapped.metadata = metadata;
        let swapped = digested(swapped);
        let mut ledger = known_ledger(&[(2, original.clone()), (20, swapped.clone())]);
        open_personal(&mut ledger);
        let swap = ledger
            .begin_hotbar_swap(InventoryTarget::Player(20), 2)
            .unwrap();
        assert!(ledger.mark_transport_enqueued(20));
        ledger.begin_world_drop(2, Some(1)).unwrap();
        ledger.begin_world_drop(2, Some(1)).unwrap();
        ledger.apply(&response(swap, StackResponseStatus::Rejected, None));
        assert!(ledger.pending_batch().unwrap().is_none());
        assert_eq!(ledger.pending_request_count(), 0);
        assert_eq!(ledger.displayed_stack(2), Some(&original));
        assert_eq!(ledger.displayed_stack(20), Some(&swapped));
        assert!(!ledger.resync_required());
    }
}
