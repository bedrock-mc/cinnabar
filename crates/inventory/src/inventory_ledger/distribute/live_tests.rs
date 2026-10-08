use super::*;

fn targets(slots: &[u8]) -> Vec<InventoryTarget> {
    slots.iter().copied().map(InventoryTarget::Player).collect()
}

#[test]
fn live_even_split_rebalances_before_any_response_with_an_empty_cursor() {
    let mut ledger = personal_ledger(&[]);
    set_cursor(&mut ledger, stack(60, 10));
    let mut drag = None;
    let first = ledger
        .advance_distribute(&mut drag, &targets(&[9, 10]), DistributeMode::Even)
        .unwrap()
        .unwrap();
    assert!(ledger.cursor_stack().is_none());
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 5);
    assert_eq!(ledger.displayed_stack(10).unwrap().count, 5);
    let second = ledger
        .advance_distribute(&mut drag, &targets(&[9, 10, 11]), DistributeMode::Even)
        .unwrap()
        .unwrap();
    assert_ne!(first, second);
    for slot in [9, 10, 11] {
        assert_eq!(ledger.displayed_stack(slot).unwrap().count, 3);
    }
    assert_eq!(ledger.cursor_stack().unwrap().count, 1);
    assert!(
        ledger
            .queue
            .iter()
            .filter(|request| request.request_id < first)
            .flat_map(|request| &request.actions)
            .any(|action| matches!(
                action, StackRequestAction::Place { source, .. } if source.stack_network_id == first
            ))
    );
    assert_eq!(
        ledger.advance_distribute(&mut drag, &targets(&[9, 10, 11]), DistributeMode::Even),
        Ok(None)
    );
}

#[test]
fn live_split_only_rebalances_its_own_items_in_preexisting_stacks() {
    let mut ledger = personal_ledger(&[(9, stack(11, 50))]);
    set_cursor(&mut ledger, stack(60, 10));
    let mut drag = None;
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10]), DistributeMode::Even)
        .unwrap();
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 55);
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10, 11]), DistributeMode::Even)
        .unwrap();
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 53);
    assert_eq!(ledger.displayed_stack(10).unwrap().count, 3);
    assert_eq!(ledger.displayed_stack(11).unwrap().count, 3);
    assert_eq!(ledger.cursor_stack().unwrap().count, 1);
}

#[test]
fn live_secondary_split_does_not_repeat_a_visited_slot() {
    let mut ledger = personal_ledger(&[]);
    set_cursor(&mut ledger, stack(60, 5));
    let mut drag = None;
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10]), DistributeMode::One)
        .unwrap();
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10, 9, 11]), DistributeMode::One)
        .unwrap();
    for slot in [9, 10, 11] {
        assert_eq!(ledger.displayed_stack(slot).unwrap().count, 1);
    }
    assert_eq!(ledger.cursor_stack().unwrap().count, 2);
}

#[test]
fn live_split_caps_merges_and_skips_an_incompatible_slot() {
    let mut incompatible = stack(21, 3);
    incompatible.metadata = 1;
    let mut ledger = personal_ledger(&[(9, stack(11, 63)), (10, incompatible.clone())]);
    set_cursor(&mut ledger, stack(60, 10));
    let mut drag = None;
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10, 11]), DistributeMode::Even)
        .unwrap();
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 64);
    assert_eq!(ledger.displayed_stack(10), Some(&incompatible));
    assert_eq!(ledger.displayed_stack(11).unwrap().count, 5);
    assert_eq!(ledger.cursor_stack().unwrap().count, 4);
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10, 11, 12]), DistributeMode::Even)
        .unwrap();
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 64);
    assert_eq!(ledger.displayed_stack(11).unwrap().count, 3);
    assert_eq!(ledger.displayed_stack(12).unwrap().count, 3);
    assert_eq!(ledger.cursor_stack().unwrap().count, 3);
}

#[test]
fn another_gesture_invalidates_split_accounting_instead_of_stealing_items() {
    let mut ledger = personal_ledger(&[]);
    set_cursor(&mut ledger, stack(60, 10));
    let mut drag = None;
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10]), DistributeMode::Even)
        .unwrap();
    ledger.begin_click(9).unwrap();
    let before = ledger.newest_request().unwrap().request_id;
    assert!(
        ledger
            .advance_distribute(&mut drag, &targets(&[9, 10, 11]), DistributeMode::Even)
            .is_err()
    );
    assert_eq!(ledger.newest_request().unwrap().request_id, before);
    assert!(ledger.displayed_stack(11).is_none());
}

#[test]
fn queue_limit_does_not_partially_submit_a_hover_rebalance() {
    let mut ledger = personal_ledger(&[]);
    set_cursor(&mut ledger, stack(60, 10));
    let mut drag = None;
    ledger
        .advance_distribute(&mut drag, &targets(&[9, 10]), DistributeMode::Even)
        .unwrap();
    let mut inactive = ledger.queue[0].clone();
    for prediction in &mut inactive.predicted {
        prediction.active = false;
    }
    while ledger.queue.len() < MAX_PENDING_REQUESTS - 1 {
        ledger.queue.push_back(inactive.clone());
    }
    let next_id = ledger.next_request_id;
    assert_eq!(
        ledger.advance_distribute(&mut drag, &targets(&[9, 10, 11]), DistributeMode::Even),
        Err(InventoryGestureError::Busy)
    );
    assert_eq!(ledger.queue.len(), MAX_PENDING_REQUESTS - 1);
    assert_eq!(ledger.next_request_id, next_id);
    assert_eq!(ledger.displayed_stack(9).unwrap().count, 5);
    assert_eq!(ledger.displayed_stack(10).unwrap().count, 5);
    assert!(ledger.displayed_stack(11).is_none());
    assert!(ledger.cursor_stack().is_none());
}
