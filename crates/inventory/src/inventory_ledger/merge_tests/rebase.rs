use super::*;

/// A response rename invalidates a queued merge with a differently named destination.
#[test]
fn review_regression_rebased_merge_checks_corrected_item_data() {
    if assets::facts::ItemFacts::builtin().is_err() {
        eprintln!("missing-fixture: builtin item facts carrier");
        return;
    }
    let mut ledger = player_ledger(
        Some(registry(vec![entry(
            6,
            "test:merge",
            Some(64),
            true,
            false,
        )])),
        stack(6, 10, 4),
        NetworkItemStack::empty(),
    );
    ledger.apply(&InventoryEvent::Slot(protocol::InventorySlotEvent {
        identity: protocol::SlotIdentity {
            container: ContainerIdentity::window(0),
            slot: 1,
        },
        stack: stack(6, 11, 2),
        storage_item: None,
    }));
    let take = ledger.begin_click(0).unwrap();
    assert!(ledger.mark_transport_enqueued(10));
    ledger.begin_click(1).unwrap();
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 6);
    ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
        responses: Arc::from([StackResponse {
            status: StackResponseStatus::Accepted,
            request_id: take,
            containers: Arc::from([StackResponseContainer {
                container: ContainerIdentity {
                    window_id: None,
                    slot_type: Some(CONTAINER_NAME_CURSOR),
                    dynamic_id: None,
                },
                slots: Arc::from([StackResponseSlot {
                    slot: 0,
                    hotbar_slot: 0,
                    count: 4,
                    item_stack_id: 91,
                    custom_name: Arc::from("Changed name"),
                    filtered_custom_name: Arc::from(""),
                    durability_correction: 0,
                }]),
            }]),
        }]),
    }));
    assert_eq!(
        ledger.pending_request_count(),
        0,
        "the now-incompatible merge is not sent"
    );
    assert_eq!(ledger.displayed_stack(1).unwrap().count, 2);
    assert_eq!(ledger.cursor_stack().unwrap().count, 4);
    assert_eq!(
        protocol::item_custom_name(&ledger.cursor_stack().unwrap().extra_data).as_deref(),
        Some("Changed name")
    );
    assert!(!ledger.resync_required());
}
