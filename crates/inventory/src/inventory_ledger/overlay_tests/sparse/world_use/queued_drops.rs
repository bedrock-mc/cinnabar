use super::*;
use protocol::test_support::transactions::observe_transaction;

/// Consumption on either side of queuing a dependent drop reaches its exact descriptors.
#[test]
fn newly_queued_world_drop_tracks_consumption_until_transport() {
    for consume_before_queue in [false, true] {
        let mut ledger = open_ledger(&[(0, stack(6, 10, 5))]);
        ledger
            .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
            .unwrap();
        send_all(&mut ledger);
        if consume_before_queue {
            consume_backing(&mut ledger, 0);
        }
        ledger.begin_world_drop(0, Some(1)).unwrap();
        consume_backing(&mut ledger, 0);
        let expected = if consume_before_queue { 2 } else { 3 };
        let (packet, _) = ledger.pending_batch().unwrap().unwrap();
        let transaction = observe_transaction(&packet).unwrap();
        assert_eq!(transaction.actions[0].from_item.count, expected);
        assert_eq!(transaction.actions[0].to_item.count, expected - 1);
        send_all(&mut ledger);
        assert_eq!(
            ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count,
            expected - 1
        );
    }
}

/// A new world drop includes consumption hidden by a transported screen prediction.
#[test]
fn newly_queued_world_drop_carries_earlier_consumption() {
    for screen_accepted in [false, true] {
        for world_accepted in [false, true] {
            for world_first in [false, true] {
                let mut ledger = open_ledger(&[(0, stack(6, 10, 5))]);
                let screen = ledger
                    .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
                    .unwrap();
                send_all(&mut ledger);
                consume_backing(&mut ledger, 0);
                assert_eq!(ledger.displayed_stack(0).unwrap().count, 4);
                let world = ledger.begin_world_drop(0, Some(1)).unwrap();
                let (packet, _) = ledger.pending_batch().unwrap().unwrap();
                let transaction = observe_transaction(&packet).unwrap();
                assert_eq!(transaction.actions[0].from_item.count, 3);
                assert_eq!(transaction.actions[0].to_item.count, 2);
                assert_eq!(transaction.actions[0].from_item.stack_network_id, None);
                send_all(&mut ledger);
                assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count, 2);
                let world = ledger
                    .queue
                    .iter()
                    .find(|request| request.request_id == world)
                    .unwrap()
                    .world_drop
                    .unwrap()
                    .legacy_request_id;
                for id in if world_first {
                    [world, screen]
                } else {
                    [screen, world]
                } {
                    let accepted = if id == screen {
                        screen_accepted
                    } else {
                        world_accepted
                    };
                    respond(
                        &mut ledger,
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
                                if id == screen { 4 } else { 2 },
                                if id == screen { 91 } else { 92 },
                            )]
                        } else {
                            vec![]
                        },
                    );
                }
                let expected = if world_accepted {
                    2
                } else if screen_accepted {
                    3
                } else {
                    4
                };
                let backing = ledger.confirmed_stack(Cell::Inventory(0)).unwrap();
                assert_eq!(backing.count, expected);
                assert_eq!(ledger.displayed_stack(0), Some(backing));
                assert!(ledger.queue.is_empty());
                assert!(!ledger.resync_required());
            }
        }
    }
}

/// All retained transfer paths attribute consumption before creating dependent transactions.
#[test]
fn newly_queued_world_drops_share_consumption_attribution() {
    #[derive(Debug, Clone, Copy)]
    enum Path {
        Drop,
        Merge,
        PartialMerge,
        Successor,
    }
    for path in [Path::Drop, Path::Merge, Path::PartialMerge, Path::Successor] {
        for used_slot in [0, 1] {
            for uses in 1_u16..=4 {
                let destination_count = if matches!(path, Path::PartialMerge) {
                    8
                } else {
                    5
                };
                let mut ledger = open_ledger(&[
                    (0, stack(6, 10, 5)),
                    (1, stack(6, 11, destination_count)),
                    (2, stack(6, 12, 7)),
                ]);
                merges::allow_merges(&mut ledger, 10);
                if matches!(path, Path::Drop) {
                    ledger
                        .begin_drop(DropSource::Target(InventoryTarget::Player(0)), Some(1))
                        .unwrap();
                } else {
                    ledger
                        .begin_quick_move_into(
                            InventoryTarget::Player(0),
                            Some(&[Cell::Inventory(1)]),
                            None,
                            |_| true,
                        )
                        .unwrap();
                }
                send_all(&mut ledger);
                if matches!(path, Path::Successor) {
                    ledger
                        .begin_quick_move_into(
                            InventoryTarget::Player(1),
                            Some(&[Cell::Inventory(2)]),
                            None,
                            |_| true,
                        )
                        .unwrap();
                    send_all(&mut ledger);
                }
                for _ in 0..uses {
                    consume_backing(&mut ledger, used_slot);
                }
                let (slot, presented, consumed) = match path {
                    Path::Drop => (0, 4, if used_slot == 0 { uses } else { 0 }),
                    Path::Merge => (1, 10, uses),
                    Path::PartialMerge => (
                        1,
                        10,
                        if used_slot == 0 {
                            uses.saturating_sub(3)
                        } else {
                            uses
                        },
                    ),
                    Path::Successor => (
                        2,
                        10,
                        if used_slot == 0 {
                            0
                        } else {
                            uses.saturating_sub(2)
                        },
                    ),
                };
                assert_eq!(ledger.displayed_stack(slot).unwrap().count, presented);
                let expected = presented - consumed;
                if expected == 0 {
                    assert_eq!(
                        ledger.begin_world_drop(slot, Some(1)),
                        Err(InventoryGestureError::EmptyGesture),
                        "{path:?}, source {used_slot}, uses {uses}"
                    );
                    continue;
                }
                ledger.begin_world_drop(slot, Some(1)).unwrap();
                let (packet, _) = ledger.pending_batch().unwrap().unwrap();
                let transaction = observe_transaction(&packet).unwrap();
                assert_eq!(
                    (
                        transaction.actions[0].from_item.count,
                        transaction.actions[0].to_item.count
                    ),
                    (expected, expected - 1),
                    "{path:?}, source {used_slot}, uses {uses}"
                );
                send_all(&mut ledger);
                assert_eq!(
                    ledger
                        .confirmed_stack(Cell::Inventory(slot))
                        .map_or(0, |stack| stack.count),
                    expected - 1,
                    "{path:?}, source {used_slot}, uses {uses}"
                );
            }
        }
    }
}
