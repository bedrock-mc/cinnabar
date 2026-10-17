//! Held stack changes publish at an eligible local tick, behind older queued movement.
use super::*;
use attack_progress_tests::{buttons, interaction_app, ticks};
use protocol::test_support::packets::equipment;

#[test]
fn held_equipment_waits_for_a_tick_and_follows_older_movement() {
    let (mut world, mut captured) = attack_hit_position_tests::attack_fixture(7.5);
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .inventory
        .publish_local_runtime_id(7, 42)
        .unwrap();
    buttons(&mut world, vec![]);
    item_use_tests::hold_snowball(&mut world);
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Slot(
            protocol::InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: protocol::ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
                    slot: 0,
                },
                stack: protocol::NetworkItemStack {
                    network_id: 7,
                    count: 1,
                    ..protocol::NetworkItemStack::empty()
                },
                storage_item: None,
            },
        ));
    let mut app = interaction_app(world);

    app.world_mut().run_schedule(Update);
    assert!(
        captured
            .drain()
            .iter()
            .all(|packet| equipment(packet).is_none())
    );

    ticks(app.world_mut(), 102, 103);
    app.world_mut().run_schedule(Update);
    let packets = captured.drain();
    let reports: Vec<_> = packets.iter().filter_map(equipment).collect();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].count, 16);
    let windows: Vec<_> = reports
        .iter()
        .map(|report| i32::from(report.container_id))
        .collect();
    assert_eq!(
        windows,
        [
            protocol::PLAYER_INVENTORY_WINDOW_ID,
            protocol::OFFHAND_WINDOW_ID
        ]
    );

    item_use_tests::hold_air_item(app.world_mut(), "minecraft:snowball", 8, [].into());
    app.world_mut().run_schedule(Update);
    assert!(
        captured
            .drain()
            .iter()
            .all(|packet| equipment(packet).is_none())
    );

    // An old input draining in a frame with no new tick must not publish this change.
    ticks(app.world_mut(), 103, 104);
    app.world_mut()
        .resource_mut::<LocalMovementEffectTimeline>()
        .begin_frame();
    app.world_mut().run_schedule(Update);
    let packets = captured.drain();
    assert!(packets.iter().all(|packet| equipment(packet).is_none()));
    assert!(packets.iter().any(|packet| {
        protocol::player_auth_input_trace_sample(packet).is_some_and(|sample| sample.tick == 103)
    }));

    // The older input was admitted before this frame's item change became eligible.
    ticks(app.world_mut(), 104, 105);
    ticks(app.world_mut(), 105, 106);
    app.world_mut().run_schedule(Update);
    let packets = captured.drain();
    let order: Vec<_> = packets
        .iter()
        .filter_map(|packet| {
            protocol::player_auth_input_trace_sample(packet)
                .map(|sample| sample.tick)
                .or_else(|| {
                    equipment(packet).map(|report| {
                        assert_eq!(report.count, 8);
                        0
                    })
                })
        })
        .collect();
    assert_eq!(order, [104, 0, 105]);

    ticks(app.world_mut(), 106, 107);
    app.world_mut().run_schedule(Update);
    assert!(
        captured
            .drain()
            .iter()
            .all(|packet| equipment(packet).is_none())
    );
}
