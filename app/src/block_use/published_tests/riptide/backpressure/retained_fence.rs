//! Inventory availability cannot remove an accepted use's release-facing fence.

use super::*;

/// Queues a real gesture that temporarily removes the selected stack from item-use facts.
fn hide_selection(world: &mut World) {
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        let ledger = ui.inventory_ledger_mut(&mut player);
        ledger.apply(&InventoryEvent::Authority(
            protocol::InventoryAuthority::Server,
        ));
        ledger.begin_world_drop(0, None).unwrap();
        assert!(ledger.slot_gesture_pending(0));
    });
    let player = world.resource::<client_state::PlayerRuntime>();
    let ui = world.resource::<UiRuntimeResource>();
    assert!(client_interaction::block_use::verified_use_selection(player, ui).is_none());
    let stream = world
        .resource::<crate::runtime::world::ClientWorld>()
        .stream
        .as_ref()
        .unwrap();
    assert!(
        client_interaction::item_use::selection::selected_riptide_use(player, stream, true)
            .is_none()
    );
}

#[test]
fn ordinary_riptide_retains_facing_when_inventory_disappears_after_release_capture() {
    for charge in [
        gameplay::item_use::RIPTIDE_CHARGE_TICKS - 1,
        gameplay::item_use::RIPTIDE_CHARGE_TICKS,
    ] {
        for wet_edge in [true, false] {
            let (mut world, mut captured) = release_edge(charge, wet_edge, true);
            let slowdown = world
                .resource::<ItemUseRuntime>()
                .movement_modifier()
                .unwrap();
            hide_selection(&mut world);
            for _ in 0..3 {
                assert!(!advance(&mut world, true).processed.spin_started);
                produce(&mut world);
                assert!(captured.drain().is_empty());
                assert_eq!(
                    world.resource::<ItemUseRuntime>().movement_modifier(),
                    Some(slowdown)
                );
            }
            flush_and_acknowledge(&mut world);
            captured.drain();
            produce(&mut world);
            assert!(
                captured.drain().is_empty(),
                "inventory unavailability cannot send the release before facing"
            );
            assert!(world.resource::<MovementTicker>().has_held_release());
            assert_eq!(
                world
                    .resource::<MovementTicker>()
                    .release_use_modifier()
                    .unwrap()
                    .1,
                slowdown
            );
            // Later look input cannot redirect the release-facing input or retained impulse.
            world
                .resource_mut::<LocalViewPose>()
                .set_rotation(Quat::IDENTITY);
            let facing = advance(&mut world, true);
            assert!(!facing.processed.spin_started);
            flush_and_acknowledge(&mut world);
            let packets = captured.drain();
            assert_eq!(packets.len(), 1);
            let input = protocol::player_auth_input_trace_sample(&packets[0]).unwrap();
            assert_eq!(input.tick, facing.tick);
            assert!((input.yaw - 90.0).abs() < 1e-3);
            assert!(!input.flag_names.contains(&"StartSpinAttack"));
            produce(&mut world);
            let packets = captured.drain();
            assert_eq!(packets.len(), 1);
            assert!(
                protocol::test_support::transactions::observe_transaction(&packets[0])
                    .unwrap()
                    .release()
                    .is_some()
            );
            let launch = advance(&mut world, true);
            let expected = wet_edge && charge == gameplay::item_use::RIPTIDE_CHARGE_TICKS;
            assert_eq!(launch.processed.spin_started, expected);
            assert_eq!(launch.movement[0] < -1.0, expected);
            assert!(!world.resource::<MovementTicker>().has_held_release());
        }
    }
}

#[test]
fn ordinary_riptide_verified_reselection_cancels_after_inventory_becomes_unavailable() {
    let (mut world, mut captured) =
        release_edge(gameplay::item_use::RIPTIDE_CHARGE_TICKS, true, true);
    // Keep another verified trident ready without observing a slot change yet.
    select_other_slot(&mut world);
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .inventory
        .set_local_selected_slot(0);
    hide_selection(&mut world);
    produce(&mut world);
    assert!(world.resource::<ItemUseRuntime>().is_using());
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .inventory
        .set_local_selected_slot(1);
    produce(&mut world);
    assert!(!world.resource::<ItemUseRuntime>().is_using());
    flush_and_acknowledge(&mut world);
    produce(&mut world);
    assert!(
        captured
            .drain()
            .iter()
            .all(|packet| protocol::player_auth_input_trace_sample(packet).is_some())
    );
    assert!(!world.resource::<MovementTicker>().has_held_release());
    assert!(!advance(&mut world, true).processed.spin_started);
}
