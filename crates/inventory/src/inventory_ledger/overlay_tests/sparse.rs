use super::*;

#[test]
fn normal_transaction_updates_backing_without_erasing_active_absolute_prediction() {
    let mut ledger = open_ledger(&[(0, stack(6, 77, 64))]);
    let request = ledger.begin_world_drop(0, Some(1)).unwrap();
    let InventoryEvent::Slot(update) = slot_push(0, stack(6, 81, 64)) else {
        panic!()
    };
    ledger.apply(&InventoryEvent::Transaction(
        protocol::InventoryTransactionEvent {
            slots: Arc::from([update]),
            skipped_actions: 0,
        },
    ));
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 63);
    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, request);
    send_all(&mut ledger);
    respond(&mut ledger, request, StackResponseStatus::Rejected, vec![]);
    assert_eq!(ledger.displayed_stack(0).unwrap(), &stack(6, 81, 64));
    let next = ledger.begin_world_drop(0, Some(1)).unwrap();
    let StackRequestAction::Drop { source, .. } = ledger.queue.back().unwrap().actions[0] else {
        panic!()
    };
    assert_eq!(
        source.stack_network_id, 81,
        "next drop uses new server identity"
    );
    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, next);
}

#[test]
fn authoritative_drop_count_is_not_decremented_a_second_time() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 64))]);
    let request = ledger.begin_world_drop(0, Some(1)).unwrap();
    send_all(&mut ledger);
    assert_eq!(count(ledger.displayed_stack(0)), Some(63));
    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, request);
    ledger.apply(&slot_push(0, stack(6, 10, 63)));
    assert_eq!(count(ledger.displayed_stack(0)), Some(63));
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            63,
            10,
        )],
    );
    assert_eq!(count(ledger.displayed_stack(0)), Some(63));
    assert_eq!(ledger.displayed_stack(0).unwrap().stack_network_id, 10);
}

#[test]
fn split_halves_and_empty_cells_chain_by_request_and_slot() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let split = ledger.begin_take_count(0, 1).unwrap();
    let place = ledger.begin_click(5).unwrap();
    let take_residual = ledger.begin_take_count(0, 1).unwrap();
    let StackRequestAction::Place { source, .. } = ledger.queue[1].actions[0] else {
        panic!()
    };
    assert_eq!(source.stack_network_id, split);
    let StackRequestAction::Take {
        source,
        destination,
        ..
    } = ledger.queue[2].actions[0]
    else {
        panic!()
    };
    assert_eq!(source.stack_network_id, split);
    assert_eq!(
        destination.stack_network_id, place,
        "empty cursor still has sparse ownership"
    );
    assert_eq!(
        ledger.cursor_stack().unwrap().stack_network_id,
        take_residual
    );
    send_all(&mut ledger);
}

#[test]
fn historic_response_updates_backing_without_erasing_newer_prediction() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let first = ledger.begin_world_drop(0, Some(1)).unwrap();
    let second = ledger.begin_world_drop(0, Some(1)).unwrap();
    send_all(&mut ledger);
    respond(
        &mut ledger,
        first,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            3,
            11,
        )],
    );
    assert_eq!(ledger.confirmed_stack(Cell::Inventory(0)).unwrap().count, 3);
    assert_eq!(count(ledger.displayed_stack(0)), Some(2));
    respond(
        &mut ledger,
        second,
        StackResponseStatus::Rejected,
        Vec::new(),
    );
    let visible = ledger.displayed_stack(0).unwrap();
    assert_eq!((visible.count, visible.stack_network_id), (3, 11));
}

#[test]
fn rejecting_latest_owner_does_not_resurrect_or_reconcile_a_missing_sparse_cell() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let first = ledger.begin_world_drop(0, Some(1)).unwrap();
    let second = ledger.begin_world_drop(0, Some(1)).unwrap();
    send_all(&mut ledger);
    respond(
        &mut ledger,
        second,
        StackResponseStatus::Rejected,
        Vec::new(),
    );
    assert_eq!(count(ledger.displayed_stack(0)), Some(4));
    respond(
        &mut ledger,
        first,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            3,
            11,
        )],
    );
    assert_eq!(count(ledger.displayed_stack(0)), Some(4));
    assert_eq!(ledger.skipped_unknown_containers(), 1);
}

#[test]
fn offhand_empty_push_before_take_response_does_not_lock_cursor_or_future_gestures() {
    for pipeline in [false, true] {
        let mut ledger = open_ledger(&[(4, stack(7, 77, 3))]);
        ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
            slots: Arc::from([stack(6, 49, 1)]),
            storage_item: NetworkItemStack::default(),
        }));
        let take = ledger
            .begin_target_gesture(InventoryTarget::Offhand, CellGesture::Click)
            .unwrap();
        let StackRequestAction::Take {
            source,
            destination,
            ..
        } = ledger.newest_action().unwrap()
        else {
            panic!("take")
        };
        assert_eq!(
            (source.container, source.slot, source.stack_network_id),
            (protocol::StackRequestContainer::Offhand, 1, 49)
        );
        assert_eq!((destination.slot, destination.stack_network_id), (0, 0));
        let mut place = pipeline.then(|| ledger.begin_click(13).unwrap());
        send_all(&mut ledger);
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
                slot: 0,
            },
            stack: NetworkItemStack::default(),
            storage_item: None,
        }));
        respond(
            &mut ledger,
            take,
            StackResponseStatus::Accepted,
            vec![
                correction(protocol::CONTAINER_NAME_OFFHAND, 1, 0, -1),
                correction(CONTAINER_NAME_CURSOR, 0, 1, 49),
            ],
        );
        assert!(
            !ledger.resync_required(),
            "accepted native offhand take never poisons cursor authority"
        );
        if !pipeline {
            assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 49);
            place = Some(ledger.begin_click(13).unwrap());
            send_all(&mut ledger);
        }
        respond(
            &mut ledger,
            place.unwrap(),
            StackResponseStatus::Accepted,
            vec![
                correction(CONTAINER_NAME_CURSOR, 0, 0, -1),
                correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 13, 1, 49),
            ],
        );
        assert_eq!(ledger.displayed_stack(13).unwrap().stack_network_id, 49);
        assert!(ledger.cursor_stack().is_none());
        assert!(ledger.target_stack(InventoryTarget::Offhand).is_none());
        assert!(
            ledger.begin_click(4).is_ok(),
            "subsequent unrelated transactions still enqueue"
        );
        send_all(&mut ledger);
    }
}

#[test]
fn response_requested_slot_selects_historic_item_not_actual_destination() {
    let mut ledger = open_ledger(&[(20, stack(6, 10, 4)), (0, stack(7, 11, 1))]);
    let request = ledger.begin_world_drop(20, Some(1)).unwrap();
    send_all(&mut ledger);
    let mut response = correction(CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY, 0, 2, 12);
    Arc::make_mut(&mut response.slots)[0].hotbar_slot = 20;
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![response],
    );
    let visible = ledger.displayed_stack(0).unwrap();
    assert_eq!(
        (visible.network_id, visible.count, visible.stack_network_id),
        (6, 2, 12)
    );
}

#[test]
fn accepted_response_uses_snapshot_even_after_backing_item_replacement() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let request = ledger.begin_world_drop(0, Some(1)).unwrap();
    send_all(&mut ledger);
    ledger.apply(&slot_push(0, stack(7, 77, 9)));
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            3,
            11,
        )],
    );
    assert_eq!(ledger.displayed_stack(0).unwrap().network_id, 6);
}

#[test]
fn malformed_count_id_pair_is_a_counted_skip_not_an_invented_stack() {
    let mut ledger = open_ledger(&[(0, stack(6, 10, 4))]);
    let request = ledger.begin_world_drop(0, Some(1)).unwrap();
    send_all(&mut ledger);
    respond(
        &mut ledger,
        request,
        StackResponseStatus::Accepted,
        vec![correction(
            CONTAINER_NAME_COMBINED_HOTBAR_AND_INVENTORY,
            0,
            3,
            0,
        )],
    );
    assert_eq!(count(ledger.displayed_stack(0)), Some(4));
    assert_eq!(ledger.skipped_unknown_containers(), 1);
}
