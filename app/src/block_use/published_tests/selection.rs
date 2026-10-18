use super::*;

#[test]
fn world_backing_recovery_allows_a_real_block_use_transaction() {
    let (mut world, mut captured) = fixture();
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let ledger = ui.inventory_ledger_mut(&mut player);
        ledger.apply(&InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(10));
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity::window(protocol::UI_INVENTORY_WINDOW_ID),
                slot: 28,
            },
            stack: protocol::NetworkItemStack {
                network_id: 6,
                count: 1,
                stack_network_id: 201,
                ..protocol::NetworkItemStack::empty()
            },
            storage_item: None,
        }));
        assert!(
            ledger.poll_timeout(10 + inventory::inventory_ledger::INVENTORY_REQUEST_TIMEOUT_MILLIS)
        );
        assert!(
            ledger.resync_required(),
            "closed input recovery remains independently retained"
        );
    });
    world.run_system_cached(produce_block_use).unwrap();
    assert_eq!(transaction_targets(&mut captured), [[4, 2, 6]]);
}

#[test]
fn direct_block_outline_respects_the_nearer_actor() {
    for (actor_z, has_outline) in [(Some(7.5), false), (Some(5.5), true), (None, true)] {
        let (mut world, _) = fixture();
        if let Some(z) = actor_z {
            actor_use_tests::spawn(&mut world, z);
        }
        let origin = frame_origin(&world, Vec3::new(4.5, 2.5, 8.5), Quat::IDENTITY);
        world.insert_resource(origin);
        world.init_resource::<client_state::RuntimeSettings>();
        world.init_resource::<render_scene::BlockSelectionFrameResource>();
        world
            .run_system_cached(crate::block_selection::publish)
            .unwrap();
        let frame = world.resource::<render_scene::BlockSelectionFrameResource>();
        assert_eq!(
            !frame.outline.is_empty(),
            has_outline,
            "actor z={actor_z:?}"
        );
    }
}
