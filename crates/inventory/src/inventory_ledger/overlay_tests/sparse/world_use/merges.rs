use super::*;

/// Installs the item capacity needed to admit compatible stack merges.
pub(super) fn allow_merges(ledger: &mut PlayerInventoryLedger, capacity: u8) {
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        capacity,
        0,
    ));
}

/// A full merge retains consumption of either input without shrinking an unrelated stack.
#[test]
fn consumption_survives_a_full_merge_response() {
    for used_slot in [0, 1] {
        for uses in 1..=5 {
            for accepted in [false, true] {
                for reverse in [false, true] {
                    let mut ledger = open_ledger(&[
                        (0, stack(6, 10, 5)),
                        (1, stack(6, 11, 5)),
                        (2, stack(6, 12, 5)),
                    ]);
                    allow_merges(&mut ledger, 64);
                    let request = ledger
                        .begin_quick_move_into(
                            InventoryTarget::Player(0),
                            Some(&[Cell::Inventory(1)]),
                            None,
                            |_| true,
                        )
                        .unwrap();
                    send_all(&mut ledger);
                    let mut consumed = 0;
                    for _ in 0..uses {
                        consumed = consume_backing(&mut ledger, used_slot);
                    }
                    assert!(ledger.displayed_stack(0).is_none());
                    assert_eq!(ledger.displayed_stack(1).unwrap().count, 10);
                    let mut results = vec![
                        correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 0, 0),
                        correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 1, 10, 91),
                    ];
                    if reverse {
                        results.reverse();
                    }
                    respond(
                        &mut ledger,
                        request,
                        if accepted {
                            StackResponseStatus::Accepted
                        } else {
                            StackResponseStatus::Rejected
                        },
                        if accepted { results } else { vec![] },
                    );
                    let source_count = if accepted {
                        0
                    } else {
                        5 - if used_slot == 0 { uses } else { 0 }
                    };
                    let destination_count = if accepted {
                        10 - uses
                    } else {
                        5 - if used_slot == 1 { uses } else { 0 }
                    };
                    for (slot, count, id) in [
                        (0, source_count, if used_slot == 0 { consumed } else { 10 }),
                        (
                            1,
                            destination_count,
                            if accepted || used_slot == 1 {
                                consumed
                            } else {
                                11
                            },
                        ),
                        (2, 5, 12),
                    ] {
                        let backing = ledger.confirmed_stack(Cell::Inventory(slot));
                        assert_eq!(backing.map_or(0, |stack| stack.count), count);
                        if let Some(backing) = backing {
                            assert_eq!(backing.stack_network_id, id);
                        }
                        assert_eq!(ledger.displayed_stack(slot), backing);
                    }
                    assert!(ledger.queue.is_empty());
                    assert!(!ledger.resync_required());
                }
            }
        }
    }
}

/// Each consumed item reduces only the destination that received its merge contribution.
#[test]
fn consumption_follows_full_merge_contributions_across_destinations() {
    for uses in 1_u16..=5 {
        let mut ledger = open_ledger(&[
            (0, stack(6, 10, 5)),
            (1, stack(6, 11, 7)),
            (2, stack(6, 12, 7)),
            (3, stack(6, 13, 5)),
        ]);
        allow_merges(&mut ledger, 8);
        let request = ledger
            .begin_quick_move_into(
                InventoryTarget::Player(0),
                Some(&[Cell::Inventory(1), Cell::Inventory(2), Cell::Inventory(3)]),
                None,
                |_| true,
            )
            .unwrap();
        send_all(&mut ledger);
        for _ in 0..uses {
            consume_backing(&mut ledger, 0);
        }
        respond(
            &mut ledger,
            request,
            StackResponseStatus::Accepted,
            (0..=3)
                .map(|slot| {
                    correction(
                        CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                        slot,
                        if slot == 0 { 0 } else { 8 },
                        if slot == 0 { 0 } else { 90 + i32::from(slot) },
                    )
                })
                .collect(),
        );
        assert!(ledger.confirmed_stack(Cell::Inventory(0)).is_none());
        for (slot, consumed) in [
            (1, uses.saturating_sub(4)),
            (2, uses.saturating_sub(3).min(1)),
            (3, uses.min(3)),
        ] {
            let backing = ledger.confirmed_stack(Cell::Inventory(slot)).unwrap();
            assert_eq!(backing.count, 8 - consumed, "slot {slot}, uses {uses}");
            assert_eq!(ledger.displayed_stack(slot), Some(backing));
        }
        assert!(ledger.queue.is_empty());
        assert!(!ledger.resync_required());
    }
}

/// Consumption follows a merged contribution through an already transported whole move.
#[test]
fn consumption_survives_a_merge_then_transported_move() {
    for source_count in [5, 6] {
        for used_slot in [0, 1] {
            for uses in 0..=5 {
                for successor_first in [false, true] {
                    let mut ledger =
                        open_ledger(&[(0, stack(6, 10, source_count)), (1, stack(6, 11, 5))]);
                    allow_merges(&mut ledger, 10);
                    let merge = ledger
                        .begin_quick_move_into(
                            InventoryTarget::Player(0),
                            Some(&[Cell::Inventory(1)]),
                            None,
                            |_| true,
                        )
                        .unwrap();
                    send_all(&mut ledger);
                    let moved = ledger
                        .begin_quick_move_into(
                            InventoryTarget::Player(1),
                            Some(&[Cell::Inventory(2)]),
                            None,
                            |_| true,
                        )
                        .unwrap();
                    send_all(&mut ledger);
                    for _ in 0..uses {
                        consume_backing(&mut ledger, used_slot);
                    }
                    for id in if successor_first {
                        [moved, merge]
                    } else {
                        [merge, moved]
                    } {
                        let results = if id == merge {
                            vec![
                                correction(
                                    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                                    0,
                                    (source_count - 5) as u8,
                                    if source_count == 5 { 0 } else { 90 },
                                ),
                                correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 1, 10, 91),
                            ]
                        } else {
                            vec![
                                correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 1, 0, 0),
                                correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 2, 10, 92),
                            ]
                        };
                        respond(&mut ledger, id, StackResponseStatus::Accepted, results);
                    }
                    let residual_use = if used_slot == 0 {
                        (source_count - 5).min(uses)
                    } else {
                        0
                    };
                    assert_eq!(
                        ledger
                            .confirmed_stack(Cell::Inventory(0))
                            .map_or(0, |s| s.count),
                        source_count - 5 - residual_use,
                    );
                    let destination = 2;
                    assert!(ledger.confirmed_stack(Cell::Inventory(1)).is_none());
                    let expected = 10 - uses + residual_use;
                    let backing = ledger
                        .confirmed_stack(Cell::Inventory(destination))
                        .unwrap();
                    assert_eq!(
                        backing.count, expected,
                        "source_count={source_count}, used_slot={used_slot}, uses={uses}, successor_first={successor_first}"
                    );
                    assert_eq!(ledger.displayed_stack(destination), Some(backing));
                    assert!(!ledger.resync_required());
                    ledger.begin_world_drop(destination, Some(1)).unwrap();
                    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
                    let transaction =
                        protocol::test_support::transactions::observe_transaction(&packet).unwrap();
                    assert_eq!(transaction.actions[0].from_item.count, expected);
                    assert_eq!(transaction.actions[0].to_item.count, expected - 1);
                    assert_eq!(transaction.actions[0].from_item.stack_network_id, None);
                }
            }
        }
    }
}

/// Consumption exhausts a partial merge's residual before reducing the transferred items.
#[test]
fn consumption_survives_a_partial_merge_response() {
    for uses in 1..=5 {
        for accepted in [false, true] {
            for reverse in [false, true] {
                let mut ledger = open_ledger(&[(0, stack(6, 10, 5)), (1, stack(6, 11, 60))]);
                allow_merges(&mut ledger, 64);
                let merge = ledger
                    .begin_quick_move_into(
                        InventoryTarget::Player(0),
                        Some(&[Cell::Inventory(1)]),
                        None,
                        |_| true,
                    )
                    .unwrap();
                send_all(&mut ledger);
                for _ in 0..uses {
                    consume_backing(&mut ledger, 0);
                }
                let mut results = vec![
                    correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 1, 90),
                    correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 1, 64, 91),
                ];
                if reverse {
                    results.reverse();
                }
                respond(
                    &mut ledger,
                    merge,
                    if accepted {
                        StackResponseStatus::Accepted
                    } else {
                        StackResponseStatus::Rejected
                    },
                    if accepted { results } else { vec![] },
                );
                assert_eq!(
                    ledger
                        .confirmed_stack(Cell::Inventory(0))
                        .map_or(0, |s| s.count),
                    if accepted { 0 } else { 5 - uses },
                );
                assert_eq!(
                    ledger.confirmed_stack(Cell::Inventory(1)).unwrap().count,
                    if accepted { 65 - uses } else { 60 },
                );
                assert_eq!(
                    ledger.displayed_stack(0),
                    ledger.confirmed_stack(Cell::Inventory(0))
                );
                assert_eq!(
                    ledger.displayed_stack(1),
                    ledger.confirmed_stack(Cell::Inventory(1))
                );
                assert!(ledger.queue.is_empty());
                assert!(!ledger.resync_required());
            }
        }
    }
}

/// A successor split preserves the position of each consumed merge contribution.
#[test]
fn consumption_follows_merged_contributions_through_a_partial_successor() {
    for used_slot in [0, 1] {
        for uses in 1_u16..=5 {
            for successor_first in [false, true] {
                let mut ledger = open_ledger(&[
                    (0, stack(6, 10, 5)),
                    (1, stack(6, 11, 5)),
                    (2, stack(6, 12, 7)),
                ]);
                allow_merges(&mut ledger, 10);
                let merge = ledger
                    .begin_quick_move_into(
                        InventoryTarget::Player(0),
                        Some(&[Cell::Inventory(1)]),
                        None,
                        |_| true,
                    )
                    .unwrap();
                send_all(&mut ledger);
                let split = ledger
                    .begin_quick_move_into(
                        InventoryTarget::Player(1),
                        Some(&[Cell::Inventory(2)]),
                        None,
                        |_| true,
                    )
                    .unwrap();
                send_all(&mut ledger);
                for _ in 0..uses {
                    consume_backing(&mut ledger, used_slot);
                }
                for id in if successor_first {
                    [split, merge]
                } else {
                    [merge, split]
                } {
                    let results = if id == merge {
                        [(0, 0, 0), (1, 10, 91)]
                    } else {
                        [(1, 7, 92), (2, 10, 93)]
                    };
                    respond(
                        &mut ledger,
                        id,
                        StackResponseStatus::Accepted,
                        results
                            .into_iter()
                            .map(|(slot, count, name)| {
                                correction(
                                    CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
                                    slot,
                                    count,
                                    name,
                                )
                            })
                            .collect(),
                    );
                }
                let residual_use = if used_slot == 0 { uses } else { uses.min(2) };
                for (slot, count) in [(1, 7 - residual_use), (2, 10 - uses + residual_use)] {
                    let backing = ledger.confirmed_stack(Cell::Inventory(slot)).unwrap();
                    assert_eq!(
                        backing.count, count,
                        "source {used_slot}, uses {uses}, slot {slot}"
                    );
                    assert_eq!(ledger.displayed_stack(slot), Some(backing));
                }
                assert!(ledger.confirmed_stack(Cell::Inventory(0)).is_none());
                assert!(ledger.queue.is_empty());
                assert!(!ledger.resync_required());
            }
        }
    }
}

/// Published drop results and following transaction descriptors retain merged consumption.
#[test]
fn consumption_reaches_a_locally_committed_merge_successor() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 5)), (1, stack(6, 11, 5))]);
    allow_merges(&mut ledger, 64);
    ledger
        .begin_quick_move_into(
            InventoryTarget::Player(0),
            Some(&[Cell::Inventory(1)]),
            None,
            |_| true,
        )
        .unwrap();
    send_all(&mut ledger);
    ledger.begin_world_drop(1, Some(1)).unwrap();
    send_all(&mut ledger);
    consume_backing(&mut ledger, 0);
    let backing = ledger.confirmed_stack(Cell::Inventory(1)).unwrap();
    assert_eq!(backing.count, 8);
    assert_eq!(ledger.displayed_stack(1), Some(backing));
    ledger.begin_world_drop(1, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    let transaction = protocol::test_support::transactions::observe_transaction(&packet).unwrap();
    assert_eq!(transaction.actions[0].from_item.count, 8);
    assert_eq!(transaction.actions[0].to_item.count, 7);
}
