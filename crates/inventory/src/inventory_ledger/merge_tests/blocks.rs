use super::*;

fn dirt_registry() -> ItemRegistryEvent {
    registry(vec![entry(6, "minecraft:dirt", None, false, true)])
}

fn block(id: i32, count: u16) -> NetworkItemStack {
    NetworkItemStack {
        block_runtime_id: 4321,
        ..ten_zero_stack(6, id, count)
    }
}

#[test]
fn accepted_block_split_can_be_restacked_without_losing_runtime_identity() {
    let mut ledger = player_ledger(Some(dirt_registry()), block(60, 32), block(61, 32));
    let request = ledger.begin_click(0).unwrap();
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 64);
    assert_eq!(ledger.displayed_stack(0).unwrap().block_runtime_id, 4321);
    assert!(ledger.cursor_stack().is_none());
    assert!(matches!(
        ledger.newest_action(),
        Some(StackRequestAction::Place { amount: 32, .. })
    ));
    assert!(ledger.mark_transport_enqueued(0));
    let InventoryEvent::Response(mut answer) = accepted_response_with_ids(request, 60, -1) else {
        panic!("response")
    };
    let corrections = Arc::make_mut(&mut Arc::make_mut(&mut answer.responses)[0].containers);
    Arc::make_mut(&mut corrections[1].slots)[0].count = 0;
    ledger.apply(&InventoryEvent::Response(answer));
    assert_eq!(ledger.displayed_stack(0), Some(&block(60, 64)));
    assert!(ledger.cursor_stack().is_none());
    // A subsequent unrelated gesture still uses the server's corrected id.
    ledger.begin_click(0).unwrap();
    assert!(
        matches!(ledger.newest_action(), Some(StackRequestAction::Take { source, .. }) if source.stack_network_id == 60)
    );
}

#[test]
fn predicted_block_split_restacks_by_request_and_slot_before_reply() {
    let mut ledger = player_ledger(
        Some(dirt_registry()),
        block(60, 64),
        NetworkItemStack::empty(),
    );
    let split = ledger.begin_take_count(0, 32).unwrap();
    ledger.begin_click(0).unwrap();
    let Some(StackRequestAction::Place {
        source,
        destination,
        amount: 32,
    }) = ledger.newest_action()
    else {
        panic!("merge place")
    };
    assert_eq!(source.stack_network_id, split);
    assert_eq!(destination.stack_network_id, split);
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 64);
    assert!(ledger.cursor_stack().is_none());
}

#[test]
fn plain_block_variants_match_aux_and_runtime_instead_of_requiring_zero() {
    let variant = |id| NetworkItemStack {
        metadata: 2,
        ..block(id, 8)
    };
    let mut ledger = player_ledger(Some(dirt_registry()), variant(60), variant(61));
    ledger.begin_click(0).unwrap();
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 16);
    assert!(super::super::registry::plain_stack(&variant(60)));
    for target in [
        NetworkItemStack {
            metadata: 1,
            ..block(60, 8)
        },
        NetworkItemStack {
            block_runtime_id: 4322,
            ..block(60, 8)
        },
    ] {
        let mut ledger = player_ledger(Some(dirt_registry()), target.clone(), block(61, 8));
        ledger.begin_click(0).unwrap();
        assert!(matches!(
            ledger.newest_action(),
            Some(StackRequestAction::Swap { .. })
        ));
        assert_eq!(
            ledger.cursor_stack(),
            Some(&NetworkItemStack {
                stack_network_id: -3,
                ..target
            })
        );
    }
}

#[test]
fn empty_user_data_encodings_do_not_prevent_block_merging() {
    let source = NetworkItemStack {
        block_runtime_id: 4321,
        ..stack(6, 61, 8)
    };
    let mut ledger = player_ledger(Some(dirt_registry()), block(60, 8), source);
    ledger.begin_click(0).unwrap();
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 16);
    assert_eq!(
        ledger.displayed_stack(0).unwrap().extra_data.as_ref(),
        &[0; 10]
    );
}
