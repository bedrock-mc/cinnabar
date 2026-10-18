use super::*;

/// Rejecting overlapping drops must preserve gameplay consumption of their retained source.
#[test]
fn consumption_survives_rejection_of_screen_and_world_drops() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 5))]);
    let screen = ledger
        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
        .unwrap();
    send_all(&mut ledger);
    let world = ledger.begin_world_drop(0, Some(1)).unwrap();
    let legacy = ledger
        .queue
        .iter()
        .find(|request| request.request_id == world)
        .unwrap()
        .world_drop
        .unwrap()
        .legacy_request_id;
    send_all(&mut ledger);
    consume_backing(&mut ledger, 0);
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count, 2);
    respond(&mut ledger, legacy, StackResponseStatus::Rejected, vec![]);
    respond(&mut ledger, screen, StackResponseStatus::Rejected, vec![]);
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count, 4);
    assert_eq!(
        ledger.displayed_stack(0),
        ledger.confirmed_stack(Cell::Inventory(0))
    );
}

/// Queues a real one-item drop and returns the identity used to answer it.
fn queue_drop(ledger: &mut PlayerInventoryLedger, world: bool) -> i32 {
    if !world {
        return ledger
            .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
            .unwrap();
    }
    let request = ledger.begin_world_drop(0, Some(1)).unwrap();
    ledger
        .queue
        .iter()
        .find(|pending| pending.request_id == request)
        .unwrap()
        .world_drop
        .unwrap()
        .legacy_request_id
}

/// Answers one drop, supplying an absolute correction only for an accepted result.
fn answer_drop(ledger: &mut PlayerInventoryLedger, id: i32, accepted: bool, count: u16) {
    respond(
        ledger,
        id,
        if accepted {
            StackResponseStatus::Accepted
        } else {
            StackResponseStatus::Rejected
        },
        if accepted {
            vec![correction(
                CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                0,
                count as u8,
                id.checked_neg().unwrap(),
            )]
        } else {
            vec![]
        },
    );
}

/// Checks final backing, presentation, and recovery after a complete set of replies.
fn assert_settled_count(ledger: &PlayerInventoryLedger, expected: u16) {
    assert_eq!(
        ledger
            .confirmed_stack(Cell::Inventory(0))
            .map_or(0, |held| held.count),
        expected
    );
    assert_eq!(
        ledger.displayed_stack(0),
        ledger.confirmed_stack(Cell::Inventory(0))
    );
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(!ledger.resync_required());
}

/// Mixed outcomes and response orders preserve consumption through empty and occupied handoffs.
#[test]
fn consumption_survives_mixed_drop_responses() {
    for initial in [3, 5] {
        for consumed in 1..=initial - 2 {
            for screen_accepted in [false, true] {
                for world_accepted in [false, true] {
                    for screen_first in [false, true] {
                        let mut ledger = open_ledger(&[(0, stack(6, 10, initial))]);
                        let screen = queue_drop(&mut ledger, false);
                        send_all(&mut ledger);
                        let world = queue_drop(&mut ledger, true);
                        send_all(&mut ledger);
                        for _ in 0..consumed {
                            consume_backing(&mut ledger, 0);
                        }
                        let replies = [
                            (screen, screen_accepted, initial - 1),
                            (world, world_accepted, initial - 2),
                        ];
                        for index in if screen_first { [0, 1] } else { [1, 0] } {
                            let (id, accepted, count) = replies[index];
                            answer_drop(&mut ledger, id, accepted, count);
                        }
                        let expected = initial
                            - consumed
                            - if world_accepted {
                                2
                            } else {
                                u16::from(screen_accepted)
                            };
                        assert_settled_count(&ledger, expected);
                    }
                }
            }
        }
    }
}

/// Unsent successors rebuild from the settled source without inheriting sent offsets twice.
#[test]
fn consumption_survives_an_unsent_drop_rebuilt_after_a_screen_reply() {
    for screen_accepted in [false, true] {
        for world_accepted in [false, true] {
            let mut ledger = open_ledger(&[(0, stack(6, 10, 5))]);
            let screen = queue_drop(&mut ledger, false);
            send_all(&mut ledger);
            let world = queue_drop(&mut ledger, true);
            consume_backing(&mut ledger, 0);
            answer_drop(&mut ledger, screen, screen_accepted, 4);
            let source_count = if screen_accepted { 3 } else { 4 };
            let (packet, _) = ledger.pending_batch().unwrap().unwrap();
            let transaction =
                protocol::test_support::transactions::observe_transaction(&packet).unwrap();
            assert_eq!(transaction.actions[0].from_item.count, source_count);
            assert_eq!(transaction.actions[0].to_item.count, source_count - 1);
            send_all(&mut ledger);
            answer_drop(&mut ledger, world, world_accepted, source_count - 1);
            assert_settled_count(&ledger, source_count - u16::from(world_accepted));
        }
    }
}

/// Rollback cannot apply consumption to another item, even when its bytes match.
#[test]
fn consumption_does_not_shrink_a_swapped_out_rollback_source() {
    for replacement_id in [6, 7] {
        for world_first in [false, true] {
            let original = stack(6, 10, 5);
            let replacement = stack(replacement_id, 11, 5);
            let mut ledger = open_ledger(&[(0, original.clone()), (1, replacement.clone())]);
            let screen = ledger
                .begin_hotbar_swap(InventoryTarget::Player(0), 1)
                .unwrap();
            send_all(&mut ledger);
            let world = queue_drop(&mut ledger, true);
            send_all(&mut ledger);
            consume_backing(&mut ledger, 0);
            for id in if world_first {
                [world, screen]
            } else {
                [screen, world]
            } {
                answer_drop(&mut ledger, id, false, 0);
            }
            assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)), Some(&original));
            assert_settled_count(&ledger, original.count);
            let mut remaining = replacement;
            remaining.count -= 1;
            assert_eq!(ledger.confirmed_stack(Cell::Inventory(1)), Some(&remaining));
            assert_eq!(ledger.displayed_stack(1), Some(&remaining));
        }
    }
}

/// Authoritative rewrites invalidate rollback history while unrelated cells retain their items.
#[test]
fn restatement_retires_consumed_rollback_history() {
    for replacement in [stack(6, 10, 5), stack(6, 11, 5), stack(7, 12, 5)] {
        for world_first in [false, true] {
            let other = stack(6, 20, 7);
            let mut ledger = open_ledger(&[(0, stack(6, 10, 5)), (1, other.clone())]);
            let screen = queue_drop(&mut ledger, false);
            send_all(&mut ledger);
            let world = queue_drop(&mut ledger, true);
            send_all(&mut ledger);
            consume_backing(&mut ledger, 0);
            ledger.apply(&slot_push(0, replacement.clone()));
            for id in if world_first {
                [world, screen]
            } else {
                [screen, world]
            } {
                answer_drop(&mut ledger, id, false, 0);
            }
            assert_eq!(
                ledger.confirmed_stack(Cell::Inventory(0)),
                Some(&replacement)
            );
            assert_eq!(ledger.confirmed_stack(Cell::Inventory(1)), Some(&other));
            assert_settled_count(&ledger, replacement.count);
        }
    }
}

/// The newest accepted absolute correction owns the count in every four-request reply order.
#[test]
fn consumption_survives_nested_mixed_drop_responses() {
    for accepted_mask in 0u8..16 {
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    if a == b || a == c || b == c {
                        continue;
                    }
                    let d = (0..4).find(|index| ![a, b, c].contains(index)).unwrap();
                    let mut ledger = open_ledger(&[(0, stack(6, 10, 7))]);
                    let ids: Vec<_> = [false, true, false, true]
                        .into_iter()
                        .map(|world| {
                            let id = queue_drop(&mut ledger, world);
                            send_all(&mut ledger);
                            id
                        })
                        .collect();
                    consume_backing(&mut ledger, 0);
                    let mut observed = Vec::new();
                    for index in [a, b, c, d] {
                        answer_drop(
                            &mut ledger,
                            ids[index],
                            accepted_mask & (1 << index) != 0,
                            6 - index as u16,
                        );
                        observed.push(
                            ledger
                                .confirmed_stack(Cell::Inventory(0))
                                .map(|held| (held.count, held.stack_network_id)),
                        );
                    }
                    let expected = (0..4)
                        .rev()
                        .find(|index| accepted_mask & (1 << index) != 0)
                        .map_or(6, |index| 5 - index as u16);
                    let held = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
                    assert_eq!(
                        held.count,
                        expected,
                        "accepted={accepted_mask:04b} order={:?} observed={observed:?}",
                        [a, b, c, d]
                    );
                    assert!(
                        held.stack_network_id > 0
                            || protocol::is_legacy_request_id(held.stack_network_id),
                        "accepted={accepted_mask:04b} order={:?} held={held:?}",
                        [a, b, c, d]
                    );
                    assert_settled_count(&ledger, expected);
                }
            }
        }
    }
}

/// Returning through an empty screen owner must not revive the item consumed after its drop.
#[test]
fn consumption_survives_take_place_and_empty_ownership_handoffs() {
    for order in [[0, 1, 2], [2, 1, 0], [1, 2, 0]] {
        let mut ledger = open_ledger(&[(0, stack(6, 10, 5))]);
        let take = ledger.begin_click(0).unwrap();
        let place = ledger.begin_click(0).unwrap();
        send_all(&mut ledger);
        let world = queue_drop(&mut ledger, true);
        send_all(&mut ledger);
        consume_backing(&mut ledger, 0);
        let ids = [take, place, world];
        for index in order {
            answer_drop(&mut ledger, ids[index], false, 0);
        }
        assert_settled_count(&ledger, 4);
        assert!(ledger.cursor_stack().is_none());
    }
}

/// A surviving correction keeps the use transaction's identity even if that use emptied backing.
#[test]
fn consumption_retains_its_identity_after_an_empty_remainder_rollback() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 3))]);
    let screen = queue_drop(&mut ledger, false);
    send_all(&mut ledger);
    let world = queue_drop(&mut ledger, true);
    send_all(&mut ledger);
    let consumption = consume_backing(&mut ledger, 0);
    assert!(ledger.confirmed_stack(Cell::Inventory(0)).is_none());
    answer_drop(&mut ledger, world, false, 0);
    answer_drop(&mut ledger, screen, true, 2);
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count, 1);
    assert_eq!(
        ledger
            .confirmed_stack(Cell::Inventory(0))
            .unwrap()
            .stack_network_id,
        consumption
    );
    assert_settled_count(&ledger, 1);
}

/// Predecessor corrections rebase retained inputs while preserving a newer backing owner.
#[test]
fn consumption_correction_reaches_an_inactive_rollback_successor() {
    for successor_accepted in [false, true] {
        let mut ledger = open_ledger(&[(0, stack(6, 10, 7))]);
        let ids: Vec<_> = [false, true, false, true]
            .into_iter()
            .map(|world| {
                let id = queue_drop(&mut ledger, world);
                send_all(&mut ledger);
                id
            })
            .collect();
        let consumption = consume_backing(&mut ledger, 0);
        answer_drop(&mut ledger, ids[1], false, 0);
        answer_drop(&mut ledger, ids[3], false, 0);
        answer_drop(&mut ledger, ids[0], true, 6);
        let backing = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
        assert_eq!((backing.count, backing.stack_network_id), (3, ids[2]));
        answer_drop(&mut ledger, ids[2], successor_accepted, 4);
        assert_settled_count(&ledger, if successor_accepted { 3 } else { 5 });
        assert_eq!(
            ledger
                .confirmed_stack(Cell::Inventory(0))
                .unwrap()
                .stack_network_id,
            consumption
        );
    }
}

/// Admitted use rebases an unsent drop after a restored source receives an accepted identity.
#[test]
fn consumption_rebuilds_an_unsent_drop_from_restored_backing() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 5))]);
    let screen = queue_drop(&mut ledger, false);
    send_all(&mut ledger);
    let previous_world = queue_drop(&mut ledger, true);
    send_all(&mut ledger);
    answer_drop(&mut ledger, previous_world, false, 0);
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count, 4);
    answer_drop(&mut ledger, screen, true, 4);
    queue_drop(&mut ledger, true);
    let consumption = consume_backing(&mut ledger, 0);
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    let transaction = protocol::test_support::transactions::observe_transaction(&packet).unwrap();
    assert_eq!(
        (
            transaction.actions[0].from_item.count,
            transaction.actions[0].from_item.stack_network_id,
            transaction.actions[0].to_item.count
        ),
        (3, Some(consumption), 2)
    );
}

/// Acknowledges a transported close while independently admitted gameplay requests remain.
fn close_use_screen(ledger: &mut PlayerInventoryLedger) {
    ledger.request_personal_close();
    send_all(ledger);
    ledger.apply(&InventoryEvent::Close(ContainerCloseEvent {
        container: ContainerIdentity::window(2),
        window_type: protocol::NO_CONTAINER_WINDOW_TYPE,
        server_initiated: false,
    }));
}

/// An admitted use resolves its exact rollback input without relying on a retiring screen alias.
#[test]
fn consumption_keeps_its_resolved_rollback_through_screen_retirement() {
    for world_first in [false, true] {
        for uses in [1, 5] {
            let original = stack(6, 10, 5);
            let other = stack(7, 11, 5);
            let mut ledger = open_ledger(&[
                (0, original.clone()),
                (1, other.clone()),
                (2, stack(8, 12, 5)),
            ]);
            ledger
                .begin_drop(DropSource::Target(InventoryTarget::Player(2)), Some(1))
                .unwrap();
            send_all(&mut ledger);
            let swap = ledger
                .begin_windowless_hotbar_swap(InventoryTarget::Player(0), 1)
                .unwrap();
            send_all(&mut ledger);
            let mut consumption = 0;
            for _ in 0..uses {
                consumption = consume_backing(&mut ledger, 0);
            }
            let world = queue_drop(&mut ledger, true);
            send_all(&mut ledger);
            close_use_screen(&mut ledger);
            for id in if world_first {
                [world, swap]
            } else {
                [swap, world]
            } {
                answer_drop(&mut ledger, id, false, 0);
            }
            let mut expected = original;
            expected.count = 5 - uses;
            expected.stack_network_id = consumption;
            let expected = (expected.count > 0).then_some(&expected);
            assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)), expected);
            assert_eq!(ledger.displayed_stack(0), expected);
            assert_eq!(ledger.confirmed_stack(Cell::Inventory(1)), Some(&other));
            assert_eq!(ledger.displayed_stack(1), Some(&other));
            assert!(!ledger.resync_required());
            ledger.begin_world_drop(1, Some(1)).unwrap();
            let (packet, _) = ledger.pending_batch().unwrap().unwrap();
            let transaction =
                protocol::test_support::transactions::observe_transaction(&packet).unwrap();
            assert_eq!(
                (
                    transaction.actions[0].from_item.count,
                    transaction.actions[0].from_item.stack_network_id
                ),
                (5, Some(11))
            );
        }
    }
}

/// Consuming a known descendant never resolves its different, unanswered rollback predecessor.
#[test]
fn consumption_preserves_unresolved_retirement_authority() {
    for initial in [3, 5] {
        for close_first in [false, true] {
            for accepted in [false, true] {
                let mut ledger = open_ledger(&[(0, stack(6, 10, initial)), (1, stack(7, 11, 4))]);
                queue_drop(&mut ledger, false);
                send_all(&mut ledger);
                let world = queue_drop(&mut ledger, true);
                send_all(&mut ledger);
                if close_first {
                    close_use_screen(&mut ledger);
                }
                let consumption = consume_backing(&mut ledger, 0);
                if !close_first {
                    close_use_screen(&mut ledger);
                }
                answer_drop(&mut ledger, world, accepted, initial - 2);
                let expected = initial - if accepted { 3 } else { 2 };
                let backing = ledger.confirmed_stack(Cell::Inventory(0));
                assert_eq!(backing.map_or(0, |stack| stack.count), expected);
                if let Some(backing) = backing {
                    assert_eq!(
                        backing.stack_network_id,
                        if accepted { consumption } else { 0 }
                    );
                }
                assert_eq!(ledger.displayed_stack(0), backing);
                assert_eq!(ledger.resync_required(), !accepted);
                assert_eq!(
                    ledger.confirmed_stack(Cell::Inventory(1)),
                    Some(&stack(7, 11, 4))
                );
                if accepted {
                    ledger.begin_world_drop(1, Some(1)).unwrap();
                    assert!(ledger.pending_batch().unwrap().is_some());
                } else {
                    assert_eq!(
                        ledger.begin_world_drop(1, Some(1)),
                        Err(InventoryGestureError::ResyncRequired)
                    );
                }
            }
        }
    }
}

/// Explicit predecessor corrections still update a locally consumed rollback before it is restored.
#[test]
fn predecessor_corrections_reach_consumed_pending_rollback_sources() {
    let original = stack(6, 10, 5);
    let other = stack(7, 11, 5);
    let mut ledger = open_ledger(&[(0, original.clone()), (1, other.clone())]);
    let screen = queue_drop(&mut ledger, false);
    send_all(&mut ledger);
    let previous_world = queue_drop(&mut ledger, true);
    send_all(&mut ledger);
    let swap = ledger
        .begin_windowless_hotbar_swap(InventoryTarget::Player(0), 1)
        .unwrap();
    send_all(&mut ledger);
    let consumption = consume_backing(&mut ledger, 0);
    let world = queue_drop(&mut ledger, true);
    send_all(&mut ledger);
    answer_drop(&mut ledger, screen, true, 2);
    answer_drop(&mut ledger, previous_world, false, 0);
    answer_drop(&mut ledger, world, false, 0);
    answer_drop(&mut ledger, swap, false, 0);
    let mut expected = original;
    expected.count = 1;
    expected.stack_network_id = consumption;
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)), Some(&expected));
    assert_eq!(ledger.displayed_stack(0), Some(&expected));
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(1)), Some(&other));
    assert!(!ledger.resync_required());
}
