use super::*;

/// Ignored ordinary descriptors cannot redirect the screen's request or response key.
#[test]
fn review_ordinary_descriptors_preserve_storage_response_correlation() {
    for name in [
        protocol::CONTAINER_NAME_CURSOR,
        protocol::CONTAINER_NAME_ARMOR,
        protocol::CONTAINER_NAME_LEVEL_ENTITY,
    ] {
        for dynamic_id in [None, Some(0), Some(9)] {
            let mut ledger = PlayerInventoryLedger::default();
            ledger.apply(&InventoryEvent::Authority(InventoryAuthority::Server));
            ledger.apply(&open(7, 0));
            let mut slots = vec![NetworkItemStack::empty(); 27];
            slots[2] = stack(5, 3, 80);
            ledger.apply(&InventoryEvent::Content(InventoryContentEvent {
                container: ContainerIdentity {
                    window_id: Some(7),
                    slot_type: Some(name),
                    dynamic_id,
                },
                slots: slots.into(),
                storage_item: NetworkItemStack::empty(),
            }));
            let request = ledger.begin_storage_click(2).unwrap();
            assert!(
                matches!(ledger.newest_action(), Some(protocol::StackRequestAction::Take { source, .. })
                if source.container == protocol::StackRequestContainer::LevelEntity { dynamic_id: None })
            );
            assert!(ledger.mark_transport_enqueued(10));
            let correction = |name, slot, count, id| StackResponseContainer {
                container: ContainerIdentity {
                    window_id: None,
                    slot_type: Some(name),
                    dynamic_id: None,
                },
                slots: Arc::from([StackResponseSlot {
                    slot,
                    hotbar_slot: slot,
                    count,
                    item_stack_id: id,
                    custom_name: Arc::from(""),
                    filtered_custom_name: Arc::from(""),
                    durability_correction: 0,
                }]),
            };
            ledger.apply(&InventoryEvent::Response(ItemStackResponseEvent {
                responses: Arc::from([StackResponse {
                    status: StackResponseStatus::Accepted,
                    request_id: request,
                    containers: Arc::from([
                        correction(protocol::CONTAINER_NAME_LEVEL_ENTITY, 2, 0, 0),
                        correction(protocol::CONTAINER_NAME_CURSOR, 0, 3, 91),
                    ]),
                }]),
            }));
            assert!(ledger.storage_stack(2).is_none());
            assert_eq!(ledger.cursor_stack(), Some(&stack(5, 3, 91)));
        }
    }
}
