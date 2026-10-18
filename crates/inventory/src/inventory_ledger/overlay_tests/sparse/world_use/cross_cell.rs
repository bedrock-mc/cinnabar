use super::*;
use crate::inventory_ledger::cells::Held;

/// Gives each source distinct retained item bytes and response metadata.
fn decorate(ledger: &mut PlayerInventoryLedger, cell: Cell, name: &str, damage: i32) -> Held {
    ledger.set_confirmed_overlay(
        cell,
        StackResponseOverlay {
            custom_name: Some(Arc::from(name)),
            filtered_custom_name: Some(Arc::from(format!("filtered {name}"))),
            durability_correction: Some(damage),
        },
    );
    ledger.confirmed.get(cell).unwrap().clone()
}

/// Checks exact item identity, NBT, response metadata, and presentation after settlement.
fn assert_cell(ledger: &PlayerInventoryLedger, cell: Cell, source: &Held, count: u16, id: i32) {
    let expected = (count > 0).then(|| {
        let mut stack = source.stack.clone();
        stack.count = count;
        stack.stack_network_id = id;
        stack
    });
    assert_eq!(ledger.confirmed_stack(cell), expected.as_ref(), "{cell:?}");
    assert_eq!(
        ledger.view().get(cell).map(|held| &held.stack),
        expected.as_ref(),
        "{cell:?}"
    );
    let overlay = (count > 0).then_some(source.overlay.as_ref()).flatten();
    assert_eq!(
        ledger
            .confirmed
            .get(cell)
            .and_then(|held| held.overlay.as_ref()),
        overlay,
        "{cell:?}"
    );
    assert_eq!(
        ledger
            .view()
            .get(cell)
            .and_then(|held| held.overlay.as_ref()),
        overlay,
        "{cell:?}"
    );
}

/// Builds an absolute response that leaves the fixture's item bytes and overlay intact.
fn result(cell: Cell, source: &Held, count: u16, id: i32) -> StackResponseContainer {
    let (container, slot) = match cell {
        Cell::Inventory(slot) => (CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, slot),
        Cell::Cursor => (CONTAINER_NAME_CURSOR, 0),
        _ => unreachable!(),
    };
    let mut response = correction(
        container,
        slot,
        count as u8,
        if count == 0 { 0 } else { id },
    );
    response.slots = Arc::from([StackResponseSlot {
        durability_correction: source
            .overlay
            .as_ref()
            .unwrap()
            .durability_correction
            .unwrap(),
        ..response.slots[0].clone()
    }]);
    response
}

/// Answers a request with its absolute pre-consumption results only when accepted.
fn answer(
    ledger: &mut PlayerInventoryLedger,
    id: i32,
    accepted: bool,
    results: Vec<StackResponseContainer>,
) {
    respond(
        ledger,
        id,
        if accepted {
            StackResponseStatus::Accepted
        } else {
            StackResponseStatus::Rejected
        },
        if accepted { results } else { vec![] },
    );
}

/// Admits a real world drop and returns its response correlation identity.
fn drop_at(ledger: &mut PlayerInventoryLedger, slot: u8) -> i32 {
    let id = ledger.begin_world_drop(slot, Some(1)).unwrap();
    send_all(ledger);
    ledger
        .queue
        .iter()
        .find(|request| request.request_id == id)
        .unwrap()
        .world_drop
        .unwrap()
        .legacy_request_id
}

/// Checks that every request has settled without introducing recovery.
fn assert_settled(ledger: &PlayerInventoryLedger) {
    assert_eq!(ledger.pending_request_count(), 0);
    assert!(ledger.queue.is_empty());
    assert!(!ledger.resync_required());
}

/// Both cells preserve the consumed source through all swap/drop outcomes and reply orders.
#[test]
fn swapped_consumption_retains_both_sources_and_overlays() {
    for replacement in [6, 7] {
        for uses in 1..=4 {
            for swap_accepted in [false, true] {
                for world_accepted in [false, true] {
                    for world_first in [false, true] {
                        let mut ledger =
                            open_ledger(&[(0, stack(6, 10, 5)), (1, stack(replacement, 11, 5))]);
                        let a = decorate(&mut ledger, Cell::Inventory(0), "first", 2);
                        let b = decorate(&mut ledger, Cell::Inventory(1), "second", 3);
                        let swap = ledger
                            .begin_hotbar_swap(InventoryTarget::Player(0), 1)
                            .unwrap();
                        send_all(&mut ledger);
                        let world = drop_at(&mut ledger, 0);
                        let mut consumed = 0;
                        for _ in 0..uses {
                            consumed = consume_backing(&mut ledger, 0);
                        }
                        for index in if world_first { [1, 0] } else { [0, 1] } {
                            if index == 0 {
                                answer(
                                    &mut ledger,
                                    swap,
                                    swap_accepted,
                                    vec![
                                        result(Cell::Inventory(0), &b, 5, 91),
                                        result(Cell::Inventory(1), &a, 5, 92),
                                    ],
                                );
                            } else {
                                answer(
                                    &mut ledger,
                                    world,
                                    world_accepted,
                                    vec![result(Cell::Inventory(0), &b, 4, 93)],
                                );
                            }
                        }
                        if world_accepted || swap_accepted {
                            assert_cell(
                                &ledger,
                                Cell::Inventory(0),
                                &b,
                                if world_accepted { 4 } else { 5 } - uses,
                                consumed,
                            );
                        } else {
                            assert_cell(&ledger, Cell::Inventory(0), &a, 5, 10);
                        }
                        if swap_accepted {
                            assert_cell(&ledger, Cell::Inventory(1), &a, 5, 92);
                        } else {
                            assert_cell(&ledger, Cell::Inventory(1), &b, 5 - uses, 11);
                        }
                        assert!(ledger.cursor_stack().is_none());
                        assert_settled(&ledger);
                    }
                }
            }
        }
    }
}

/// Consumption of backing A belongs to A's moved result, never to replacement B at the use cell.
#[test]
fn swap_corrections_follow_consumption_to_its_destination() {
    for uses in 1..=5 {
        for accepted in [false, true] {
            let mut ledger = open_ledger(&[(0, stack(6, 10, 5)), (1, stack(6, 11, 5))]);
            let a = decorate(&mut ledger, Cell::Inventory(0), "first", 2);
            let b = decorate(&mut ledger, Cell::Inventory(1), "second", 3);
            let swap = ledger
                .begin_hotbar_swap(InventoryTarget::Player(0), 1)
                .unwrap();
            send_all(&mut ledger);
            let mut consumed = 0;
            for _ in 0..uses {
                consumed = consume_backing(&mut ledger, 0);
            }
            answer(
                &mut ledger,
                swap,
                accepted,
                vec![
                    result(Cell::Inventory(0), &b, 5, 91),
                    result(Cell::Inventory(1), &a, 5, 92),
                ],
            );
            if accepted {
                assert_cell(&ledger, Cell::Inventory(0), &b, 5, 91);
                assert_cell(&ledger, Cell::Inventory(1), &a, 5 - uses, consumed);
            } else {
                assert_cell(&ledger, Cell::Inventory(0), &a, 5 - uses, consumed);
                assert_cell(&ledger, Cell::Inventory(1), &b, 5, 11);
            }
            assert_settled(&ledger);
        }
    }
}

/// Cursor moves retain one source through each response permutation without double subtraction.
#[test]
fn consumption_follows_take_and_place_across_cells() {
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        for uses in 1..=4 {
            for moves_accepted in [false, true] {
                for world_accepted in [false, true] {
                    let mut ledger = open_ledger(&[(1, stack(6, 11, 5))]);
                    let source = decorate(&mut ledger, Cell::Inventory(1), "moved", 3);
                    let take = ledger.begin_click(1).unwrap();
                    send_all(&mut ledger);
                    let place = ledger.begin_click(0).unwrap();
                    send_all(&mut ledger);
                    let world = drop_at(&mut ledger, 0);
                    let mut consumed = 0;
                    for _ in 0..uses {
                        consumed = consume_backing(&mut ledger, 0);
                    }
                    for index in order {
                        let (id, accepted, results) = match index {
                            0 => (
                                take,
                                moves_accepted,
                                vec![
                                    result(Cell::Inventory(1), &source, 0, 0),
                                    result(Cell::Cursor, &source, 5, 91),
                                ],
                            ),
                            1 => (
                                place,
                                moves_accepted,
                                vec![
                                    result(Cell::Cursor, &source, 0, 0),
                                    result(Cell::Inventory(0), &source, 5, 92),
                                ],
                            ),
                            _ => (
                                world,
                                world_accepted,
                                vec![result(Cell::Inventory(0), &source, 4, 93)],
                            ),
                        };
                        answer(&mut ledger, id, accepted, results);
                    }
                    if moves_accepted || world_accepted {
                        assert_cell(
                            &ledger,
                            Cell::Inventory(0),
                            &source,
                            if world_accepted { 4 } else { 5 } - uses,
                            consumed,
                        );
                    } else {
                        assert!(ledger.confirmed_stack(Cell::Inventory(0)).is_none());
                        assert!(ledger.displayed_stack(0).is_none());
                    }
                    assert_cell(
                        &ledger,
                        Cell::Inventory(1),
                        &source,
                        if moves_accepted { 0 } else { 5 - uses },
                        11,
                    );
                    assert!(ledger.confirmed_stack(Cell::Cursor).is_none());
                    assert!(ledger.cursor_stack().is_none());
                    assert_settled(&ledger);
                }
            }
        }
    }
}

/// Sibling split outputs share a request wire name but retain distinct source identities.
#[test]
fn consuming_a_split_output_does_not_shrink_its_sibling() {
    let mut ledger = open_ledger(&[(9, stack(6, 10, 10))]);
    ledger.apply_registry(&protocol::test_support::items::component_item_registry(
        "minecraft:apple",
        6,
        3,
        0,
    ));
    let source = decorate(&mut ledger, Cell::Inventory(9), "split", 3);
    let split = ledger
        .begin_quick_move_into(
            InventoryTarget::Player(9),
            Some(&[Cell::Inventory(0), Cell::Inventory(1), Cell::Inventory(2)]),
            Some(9),
            |_| true,
        )
        .unwrap();
    for slot in [0, 1, 2] {
        assert_eq!(
            ledger.displayed_stack(slot).unwrap().stack_network_id,
            split
        );
        assert_eq!(ledger.displayed_stack(slot).unwrap().count, 3);
    }
    send_all(&mut ledger);
    let first = drop_at(&mut ledger, 0);
    let sibling = drop_at(&mut ledger, 1);
    let consumed = consume_backing(&mut ledger, 0);
    assert_cell(&ledger, Cell::Inventory(0), &source, 1, consumed);
    assert_cell(&ledger, Cell::Inventory(1), &source, 2, sibling);
    answer(&mut ledger, first, false, vec![]);
    answer(&mut ledger, sibling, false, vec![]);
    let expected_sibling = ledger.displayed_stack(1).unwrap();
    assert_eq!(expected_sibling.count, 3);
    assert_eq!(
        ledger.confirmed_stack(Cell::Inventory(1)),
        Some(expected_sibling)
    );
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count, 2);
    answer(
        &mut ledger,
        split,
        true,
        vec![
            result(Cell::Inventory(0), &source, 3, 91),
            result(Cell::Inventory(1), &source, 3, 92),
            result(Cell::Inventory(2), &source, 3, 93),
            result(Cell::Inventory(9), &source, 1, 94),
        ],
    );
    assert_cell(&ledger, Cell::Inventory(0), &source, 2, consumed);
    assert_cell(&ledger, Cell::Inventory(1), &source, 3, 92);
    assert_cell(&ledger, Cell::Inventory(2), &source, 3, 93);
    assert_cell(&ledger, Cell::Inventory(9), &source, 1, 94);
    assert_settled(&ledger);
}

/// Unassigned wire names do not make independent, byte-identical backing stacks aliases.
#[test]
fn consuming_an_unnamed_stack_does_not_shrink_another_cell() {
    let original = stack(6, -1, 5);
    let mut ledger = open_ledger(&[(0, original.clone()), (1, original.clone())]);
    let consumed = consume_backing(&mut ledger, 0);
    let mut remainder = original.clone();
    remainder.count -= 1;
    remainder.stack_network_id = consumed;
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)), Some(&remainder));
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(1)), Some(&original));
    assert_eq!(ledger.displayed_stack(0), Some(&remainder));
    assert_eq!(ledger.displayed_stack(1), Some(&original));
    assert_settled(&ledger);
}

/// Independent uses before and after a swap is published stay attached to their own items.
#[test]
fn consecutive_sources_at_one_use_cell_keep_separate_consumption() {
    for swap_accepted in [false, true] {
        for world_accepted in [false, true] {
            for world_first in [false, true] {
                let mut ledger = open_ledger(&[(0, stack(6, 10, 5)), (1, stack(6, 11, 5))]);
                let a = decorate(&mut ledger, Cell::Inventory(0), "first", 2);
                let b = decorate(&mut ledger, Cell::Inventory(1), "second", 3);
                let swap = ledger
                    .begin_hotbar_swap(InventoryTarget::Player(0), 1)
                    .unwrap();
                send_all(&mut ledger);
                let use_a = consume_backing(&mut ledger, 0);
                let world = drop_at(&mut ledger, 0);
                let use_b = consume_backing(&mut ledger, 0);
                for index in if world_first { [1, 0] } else { [0, 1] } {
                    if index == 0 {
                        answer(
                            &mut ledger,
                            swap,
                            swap_accepted,
                            vec![
                                result(Cell::Inventory(0), &b, 5, 91),
                                result(Cell::Inventory(1), &a, 5, 92),
                            ],
                        );
                    } else {
                        answer(
                            &mut ledger,
                            world,
                            world_accepted,
                            vec![result(Cell::Inventory(0), &b, 4, 93)],
                        );
                    }
                }
                if world_accepted || swap_accepted {
                    assert_cell(
                        &ledger,
                        Cell::Inventory(0),
                        &b,
                        if world_accepted { 3 } else { 4 },
                        use_b,
                    );
                } else {
                    assert_cell(&ledger, Cell::Inventory(0), &a, 4, use_a);
                }
                if swap_accepted {
                    assert_cell(&ledger, Cell::Inventory(1), &a, 4, use_a);
                } else {
                    assert_cell(&ledger, Cell::Inventory(1), &b, 4, 11);
                }
                assert_settled(&ledger);
            }
        }
    }
}

/// Rewriting the old source cell severs that rollback edge without changing another source.
#[test]
fn cross_cell_consumption_does_not_follow_an_authoritative_replacement() {
    for world_first in [false, true] {
        let mut ledger = open_ledger(&[(0, stack(6, 10, 5)), (1, stack(6, 11, 5))]);
        let a = decorate(&mut ledger, Cell::Inventory(0), "first", 2);
        decorate(&mut ledger, Cell::Inventory(1), "second", 3);
        let swap = ledger
            .begin_hotbar_swap(InventoryTarget::Player(0), 1)
            .unwrap();
        send_all(&mut ledger);
        let world = drop_at(&mut ledger, 0);
        ledger.apply(&slot_push(1, stack(6, 12, 7)));
        let replacement = decorate(&mut ledger, Cell::Inventory(1), "replacement", 4);
        consume_backing(&mut ledger, 0);
        for id in if world_first {
            [world, swap]
        } else {
            [swap, world]
        } {
            answer(&mut ledger, id, false, vec![]);
        }
        assert_cell(&ledger, Cell::Inventory(0), &a, 5, 10);
        assert_cell(&ledger, Cell::Inventory(1), &replacement, 7, 12);
        assert_settled(&ledger);
    }
}
