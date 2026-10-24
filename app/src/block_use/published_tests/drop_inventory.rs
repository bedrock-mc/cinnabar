use super::*;

#[test]
fn a_drop_after_an_admitted_placement_uses_the_remaining_block_count() {
    let (mut world, mut captured) =
        floor_fixture([4.5, 2.620_01, 8.32], Quat::from_rotation_x(-0.9601));
    {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let ledger = player.inventory.ledger_mut();
        ledger.apply(&InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
        let mut stack = ledger.displayed_stack(0).unwrap().clone();
        stack.count = 3;
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(0),
                slot: 0,
            },
            stack,
            storage_item: None,
        }));
    }
    let mut router = crate::semantic_controls::SemanticInputRuntime::default();
    hold(&mut world, &mut router, Vec::new(), vec![2]);
    world.run_system_cached(produce_block_use).unwrap();
    let packets = captured.drain();
    let placement = packets
        .iter()
        .filter_map(protocol::test_support::transactions::observe_transaction)
        .find(|transaction| {
            transaction.actions.iter().any(|action| {
                action.source
                    == protocol::test_support::transactions::InventorySourceKind::Container
            })
        })
        .expect("an admitted placement has a count change");
    assert!(protocol::is_legacy_request_id(placement.legacy_request_id));
    let mut player = world.resource_mut::<client_state::PlayerRuntime>();
    let ledger = player.inventory.ledger_mut();
    assert_eq!(ledger.displayed_stack(0).unwrap().count, 2);
    ledger.begin_world_drop(0, Some(1)).unwrap();
    let (packet, _) = ledger.pending_batch().unwrap().unwrap();
    let drop = protocol::test_support::transactions::observe_transaction(&packet).unwrap();
    assert_ne!(drop.legacy_request_id, placement.legacy_request_id);
    assert_eq!(drop.actions[0].from_item.count, 2);
    assert_eq!(drop.actions[0].from_item.stack_network_id, None);
    assert_eq!(drop.actions[0].to_item.count, 1);
    assert_eq!(drop.actions[0].to_item.stack_network_id, None);
}
