//! Creative catalog result declarations and transfers.
use super::*;

/// Installs an original catalog entry for creative gestures.
fn creative(ledger: &mut PlayerInventoryLedger) {
    ledger.set_permissions(InventoryPermissions {
        creative: true,
        instabuild: Some(true),
    });
    ledger.apply(&InventoryEvent::Creative(protocol::CreativeContentEvent {
        groups: Arc::from([]),
        items: Arc::from([protocol::CreativeItem {
            creative_network_id: 44,
            stack: stack(COBBLE, -1, 1),
            group: 0,
        }]),
        skipped: 0,
    }));
}

/// A creative take crafts the entry and transfers a full stack out of created
/// output, named by the request id.
#[test]
fn creative_take_moves_a_full_stack_into_the_cursor() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    creative(&mut ledger);
    let request = ledger
        .begin_creative_take(44, CreativeDestination::Cursor, None)
        .unwrap();
    let actions = &ledger.newest_request().unwrap().actions;
    assert!(matches!(
        actions[0],
        StackRequestAction::CraftCreative {
            creative_item_network_id: 44,
            crafts: 1
        }
    ));
    assert!(matches!(
        &actions[1],
        StackRequestAction::CraftResultsDeprecated { results, crafts: 1 }
            if results.as_ref() == [protocol::CraftResult {
                identifier: Arc::from("minecraft:cobblestone"),
                aux: 0,
                count: 1,
                block_runtime_id: 0,
                user_data: Arc::from([]),
            }]
    ));
    let StackRequestAction::Take {
        amount,
        source,
        destination,
    } = actions[2]
    else {
        panic!("take into the cursor");
    };
    assert_eq!(
        (amount, source.slot, source.stack_network_id),
        (64, 50, request)
    );
    assert_eq!(destination.container, StackRequestContainer::Cursor);
    assert!(ledger.pending_batch().unwrap().is_some());
    let held = ledger.cursor_stack().unwrap();
    assert_eq!((held.network_id, held.count), (COBBLE, 64));

    assert!(ledger.mark_transport_enqueued(10));
    respond(&mut ledger, request, &[(CONTAINER_NAME_CURSOR, 0, 64, 700)]);
    assert_eq!(ledger.cursor_stack().unwrap().stack_network_id, 700);
    assert!(!ledger.resync_required());
}

#[test]
fn creative_take_refuses_unparsed_stack_size_data() {
    for unrelated_depth in [0, 40] {
        let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
        let registry = protocol::test_support::items::component_item_registry(
            "minecraft:cobblestone",
            COBBLE as i16,
            1,
            unrelated_depth,
        );
        ledger.apply_registry(&registry);
        creative(&mut ledger);
        if unrelated_depth == 0 {
            ledger
                .begin_creative_take(44, CreativeDestination::Cursor, None)
                .unwrap();
            assert_eq!(ledger.cursor_stack().unwrap().count, 1);
            continue;
        }
        for destination in [
            CreativeDestination::Cursor,
            CreativeDestination::Player(4),
            CreativeDestination::Drop,
        ] {
            assert_eq!(
                ledger.begin_creative_take(44, destination, None),
                Err(InventoryGestureError::InvalidRequest)
            );
            assert_eq!(ledger.pending_request_count(), 0);
            assert!(ledger.cursor_stack().is_none());
            assert!(ledger.displayed_stack(4).is_none());
        }
    }
}

/// Vanilla's creative create-item scope declares
/// the catalog prototype, not the full-stack prediction used for the transfer.
#[test]
fn creative_take_declares_the_catalog_item_metadata_and_user_data() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    creative(&mut ledger);
    let prototype = NetworkItemStack {
        metadata: 2,
        block_runtime_id: 123,
        extra_data: Arc::from([0; 10]),
        ..stack(COBBLE, -1, 1)
    };
    ledger.apply(&InventoryEvent::Creative(protocol::CreativeContentEvent {
        groups: Arc::from([]),
        items: Arc::from([protocol::CreativeItem {
            creative_network_id: 44,
            stack: prototype.clone(),
            group: 0,
        }]),
        skipped: 0,
    }));
    ledger
        .begin_creative_take(44, CreativeDestination::Player(4), None)
        .unwrap();
    let StackRequestAction::CraftResultsDeprecated { results, crafts } =
        &ledger.newest_request().unwrap().actions[1]
    else {
        panic!("creative take declares its result");
    };
    assert_eq!(*crafts, 1);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].identifier.as_ref(), "minecraft:cobblestone");
    assert_eq!(results[0].aux, prototype.metadata as i32);
    assert_eq!(results[0].count, prototype.count);
    assert_eq!(
        results[0].block_runtime_id,
        prototype.block_runtime_id as u32
    );
    assert_eq!(results[0].user_data, prototype.extra_data);
    assert_eq!(ledger.displayed_stack(4).unwrap().count, 64);
}

#[test]
fn creative_take_preserves_high_bit_block_ids_and_signed_aux() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    creative(&mut ledger);
    let prototype = NetworkItemStack {
        metadata: u32::MAX,
        block_runtime_id: i32::MIN + 123,
        ..stack(COBBLE, -1, 1)
    };
    ledger.apply(&InventoryEvent::Creative(protocol::CreativeContentEvent {
        groups: Arc::from([]),
        items: Arc::from([protocol::CreativeItem {
            creative_network_id: 44,
            stack: prototype.clone(),
            group: 0,
        }]),
        skipped: 0,
    }));
    ledger
        .begin_creative_take(44, CreativeDestination::Cursor, None)
        .unwrap();
    let StackRequestAction::CraftResultsDeprecated { results, .. } =
        &ledger.newest_request().unwrap().actions[1]
    else {
        panic!("declared creative prototype")
    };
    assert_eq!(results[0].aux, -1);
    assert_eq!(
        results[0].block_runtime_id,
        u32::from_ne_bytes(prototype.block_runtime_id.to_ne_bytes())
    );
    assert_eq!(
        ledger.cursor_stack().unwrap().block_runtime_id,
        prototype.block_runtime_id
    );
}

#[test]
fn creative_drop_creates_one_or_a_whole_stack_without_occupying_the_cursor() {
    for (whole_stack, expected) in [(false, 1), (true, protocol::ITEM_DEFAULT_MAX_STACK_SIZE)] {
        let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
        creative(&mut ledger);
        let request = ledger
            .begin_creative_take(44, CreativeDestination::Drop, (!whole_stack).then_some(1))
            .unwrap();
        let actions = &ledger.newest_request().unwrap().actions;
        assert!(matches!(
            actions[0],
            StackRequestAction::CraftCreative {
                creative_item_network_id: 44,
                crafts: 1
            }
        ));
        assert!(
            matches!(actions[2], StackRequestAction::Drop { amount, source, randomly: false } if amount == expected && source.container == StackRequestContainer::CreatedOutput && source.stack_network_id == request)
        );
        assert!(ledger.cursor_stack().is_none());
        assert!(
            ledger
                .view()
                .get(super::cells::Cell::CreatedOutput)
                .is_none()
        );
        assert!(ledger.pending_batch().unwrap().is_some());
    }
}

/// Unknown entries and occupied destinations send nothing.
#[test]
fn creative_take_refuses_unknown_entries_and_occupied_destinations() {
    let mut ledger = ledger(PERSONAL_INVENTORY_WINDOW_TYPE);
    assert_eq!(
        ledger.begin_creative_take(44, CreativeDestination::Cursor, None),
        Err(InventoryGestureError::InvalidRequest)
    );
    creative(&mut ledger);
    assert_eq!(
        ledger.begin_creative_take(45, CreativeDestination::Cursor, None),
        Err(InventoryGestureError::InvalidRequest)
    );
    ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
        identity: SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 3,
        },
        stack: stack(LOG, 90, 1),
        storage_item: None,
    }));
    assert_eq!(
        ledger.begin_creative_take(44, CreativeDestination::Player(3), None),
        Err(InventoryGestureError::InvalidRequest)
    );
    assert_eq!(ledger.pending_request_count(), 0);
    let request = ledger
        .begin_creative_take(44, CreativeDestination::Player(4), None)
        .unwrap();
    assert!(matches!(
        ledger.newest_request().unwrap().actions[2],
        StackRequestAction::Place { amount: 64, .. }
    ));
    assert_eq!(ledger.displayed_stack(4).unwrap().stack_network_id, request);
}
