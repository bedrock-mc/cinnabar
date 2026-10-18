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
        .apply(&protocol::InventoryEvent::Content(
            protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
                slots: [protocol::NetworkItemStack {
                    network_id: 7,
                    count: 1,
                    ..protocol::NetworkItemStack::empty()
                }]
                .into(),
                storage_item: protocol::NetworkItemStack::empty(),
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

const RETRY_PAYLOAD_CAPACITY: usize = 8;

#[derive(Debug, PartialEq, Eq)]
enum RetryEvent {
    Main(u16),
    Off(u16),
    Input(u64),
}

/// Observes equipment and movement admission while excluding filler packets.
fn retry_events(packets: &[protocol::Packet]) -> Vec<RetryEvent> {
    packets
        .iter()
        .filter_map(|packet| {
            if let Some(report) = equipment(packet) {
                return Some(match i32::from(report.container_id) {
                    protocol::PLAYER_INVENTORY_WINDOW_ID => RetryEvent::Main(report.count),
                    protocol::OFFHAND_WINDOW_ID => RetryEvent::Off(report.count),
                    other => panic!("unexpected equipment container {other}"),
                });
            }
            protocol::player_auth_input_trace_sample(packet)
                .map(|sample| RetryEvent::Input(sample.tick))
        })
        .collect()
}

/// Changes known backing counts without changing held item identity.
fn retry_counts(world: &mut World, main: u16, off: u16) {
    let mut player = world.resource_mut::<client_state::PlayerRuntime>();
    let mut stack = player.selected_stack().unwrap().clone();
    stack.count = main;
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Slot(
            protocol::InventorySlotEvent {
                identity: protocol::SlotIdentity {
                    container: protocol::ContainerIdentity::window(
                        protocol::PLAYER_INVENTORY_WINDOW_ID,
                    ),
                    slot: 0,
                },
                stack: stack.clone(),
                storage_item: None,
            },
        ));
    stack.count = off;
    player
        .inventory
        .ledger_mut()
        .apply(&protocol::InventoryEvent::Content(
            protocol::InventoryContentEvent {
                container: protocol::ContainerIdentity::window(protocol::OFFHAND_WINDOW_ID),
                slots: [stack].into(),
                storage_item: protocol::NetworkItemStack::empty(),
            },
        ));
}

/// Establishes accepted baseline equipment through the production schedule.
fn retry_app() -> (App, client_session::CapturedPackets) {
    let (network, captured) =
        client_session::NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(
            RETRY_PAYLOAD_CAPACITY,
        );
    let transport = (
        NetworkHandle::connected(network, Default::default()),
        captured,
    );
    let (mut world, mut captured) = fixture_with_network(transport);
    buttons(&mut world, vec![]);
    item_use_tests::hold_snowball(&mut world);
    world
        .resource_mut::<client_state::PlayerRuntime>()
        .inventory
        .publish_local_runtime_id(7, 42)
        .unwrap();
    retry_counts(&mut world, 16, 1);
    ticks(&mut world, 102, 103);
    let mut app = interaction_app(world);
    app.world_mut().run_schedule(Update);

    assert_eq!(
        retry_events(&captured.drain()),
        [
            RetryEvent::Input(101),
            RetryEvent::Main(16),
            RetryEvent::Off(1),
            RetryEvent::Input(102),
        ],
        "fixture must establish the baseline before the regression assertion",
    );
    (app, captured)
}

/// Leaves exactly the requested number of payload admissions available.
fn retry_leave_capacity(app: &App, available: usize) {
    assert!(available <= RETRY_PAYLOAD_CAPACITY);
    let network = app.world().resource::<NetworkHandle<PackApplication>>();
    for _ in available..RETRY_PAYLOAD_CAPACITY {
        network
            .send_inventory_packet(protocol::swing_arm_packet(42, protocol::SwingSource::Mine))
            .unwrap();
    }
}

/// Runs another sender frame without committing a physics tick.
fn retry_without_tick(app: &mut App) {
    app.world_mut()
        .resource_mut::<LocalMovementEffectTimeline>()
        .begin_frame();
    app.world_mut().run_schedule(Update);
}

/// Exercises partial admission at the real sender and transport boundaries.
fn retry_boundary(available: usize, first: &[RetryEvent], retry: &[RetryEvent]) {
    let (mut app, mut captured) = retry_app();
    retry_counts(app.world_mut(), 8, 2);
    ticks(app.world_mut(), 103, 104);
    retry_leave_capacity(&app, available);
    app.world_mut().run_schedule(Update);

    assert_eq!(retry_events(&captured.drain()), first);
    assert!(
        app.world()
            .resource::<MovementTicker>()
            .unsent_sample_at(103)
            .is_some(),
        "the refused movement sample must remain available",
    );
    assert!(
        app.world()
            .resource::<crate::runtime::shutdown::FatalError>()
            .0
            .is_none(),
    );

    retry_without_tick(&mut app);
    assert_eq!(retry_events(&captured.drain()), retry);
    assert!(
        app.world()
            .resource::<MovementTicker>()
            .unsent_sample_at(103)
            .is_none(),
    );

    retry_without_tick(&mut app);
    assert!(retry_events(&captured.drain()).is_empty());
}

#[test]
fn held_equipment_retry_main_full_keeps_both_hands_before_input() {
    retry_boundary(
        0,
        &[],
        &[
            RetryEvent::Main(8),
            RetryEvent::Off(2),
            RetryEvent::Input(103),
        ],
    );
}

#[test]
fn held_equipment_retry_offhand_full_does_not_repeat_mainhand() {
    retry_boundary(
        1,
        &[RetryEvent::Main(8)],
        &[RetryEvent::Off(2), RetryEvent::Input(103)],
    );
}

#[test]
fn held_equipment_retry_movement_full_does_not_repeat_equipment() {
    retry_boundary(
        2,
        &[RetryEvent::Main(8), RetryEvent::Off(2)],
        &[RetryEvent::Input(103)],
    );
}

#[test]
fn held_equipment_retry_does_not_make_a_later_hand_eligible() {
    let (mut app, mut captured) = retry_app();
    retry_counts(app.world_mut(), 8, 1);
    ticks(app.world_mut(), 103, 104);
    retry_leave_capacity(&app, 0);
    app.world_mut().run_schedule(Update);
    assert!(retry_events(&captured.drain()).is_empty());

    retry_counts(app.world_mut(), 8, 2);
    retry_without_tick(&mut app);
    assert_eq!(
        retry_events(&captured.drain()),
        [RetryEvent::Main(8), RetryEvent::Input(103)],
    );

    ticks(app.world_mut(), 104, 105);
    app.world_mut().run_schedule(Update);
    assert_eq!(
        retry_events(&captured.drain()),
        [RetryEvent::Off(2), RetryEvent::Input(104)],
    );
}

#[test]
fn held_equipment_retry_coalesces_and_acknowledges_current_backing() {
    let (mut app, mut captured) = retry_app();
    retry_counts(app.world_mut(), 8, 1);
    ticks(app.world_mut(), 103, 104);
    retry_leave_capacity(&app, 0);
    app.world_mut().run_schedule(Update);
    assert!(retry_events(&captured.drain()).is_empty());

    retry_counts(app.world_mut(), 4, 1);
    retry_without_tick(&mut app);
    assert_eq!(
        retry_events(&captured.drain()),
        [RetryEvent::Main(4), RetryEvent::Input(103)],
    );
    let player = app.world().resource::<client_state::PlayerRuntime>();
    assert!(
        player
            .inventory
            .changed_held_equipment_packet(player.facts.player_game_mode())
            .unwrap()
            .is_none(),
    );
}

#[test]
fn held_equipment_retry_does_not_reuse_an_admitted_hands_eligibility() {
    let (mut app, mut captured) = retry_app();
    retry_counts(app.world_mut(), 8, 2);
    ticks(app.world_mut(), 103, 104);
    retry_leave_capacity(&app, 1);
    app.world_mut().run_schedule(Update);
    assert_eq!(retry_events(&captured.drain()), [RetryEvent::Main(8)]);

    retry_counts(app.world_mut(), 4, 2);
    retry_without_tick(&mut app);
    assert_eq!(
        retry_events(&captured.drain()),
        [RetryEvent::Off(2), RetryEvent::Input(103)],
    );
    ticks(app.world_mut(), 104, 105);
    app.world_mut().run_schedule(Update);
    assert_eq!(
        retry_events(&captured.drain()),
        [RetryEvent::Main(4), RetryEvent::Input(104)],
    );
}

#[test]
fn held_equipment_retry_cancels_when_the_announced_stack_returns() {
    let (mut app, mut captured) = retry_app();
    retry_counts(app.world_mut(), 8, 1);
    ticks(app.world_mut(), 103, 104);
    retry_leave_capacity(&app, 0);
    app.world_mut().run_schedule(Update);
    assert!(retry_events(&captured.drain()).is_empty());

    retry_counts(app.world_mut(), 16, 1);
    retry_without_tick(&mut app);
    assert_eq!(retry_events(&captured.drain()), [RetryEvent::Input(103)]);

    retry_counts(app.world_mut(), 4, 1);
    ticks(app.world_mut(), 104, 105);
    retry_without_tick(&mut app);
    assert_eq!(retry_events(&captured.drain()), [RetryEvent::Input(104)]);
    ticks(app.world_mut(), 105, 106);
    app.world_mut().run_schedule(Update);
    assert_eq!(
        retry_events(&captured.drain()),
        [RetryEvent::Main(4), RetryEvent::Input(105)],
    );
}

#[test]
fn held_equipment_retry_retires_eligibility_with_position_authority() {
    let position = [4.5, 2.620_01, 8.5];
    for reset in [false, true] {
        let (mut app, mut captured) = retry_app();
        // Retire warm-up socket ownership before exercising the scope change.
        app.world_mut()
            .resource_mut::<MovementTicker>()
            .reset(7, 102, position);
        retry_without_tick(&mut app);
        assert!(retry_events(&captured.drain()).is_empty());

        retry_counts(app.world_mut(), 8, 1);
        ticks(app.world_mut(), 103, 104);
        retry_leave_capacity(&app, 0);
        app.world_mut().run_schedule(Update);
        assert!(retry_events(&captured.drain()).is_empty());
        {
            let mut movement = app.world_mut().resource_mut::<MovementTicker>();
            if reset {
                movement.reset(7, 103, position);
            } else {
                movement.reanchor_surface_spawn(103, position);
            }
        }
        ticks(app.world_mut(), 104, 105);
        retry_without_tick(&mut app);
        assert_eq!(retry_events(&captured.drain()), [RetryEvent::Input(104)]);

        ticks(app.world_mut(), 105, 106);
        app.world_mut().run_schedule(Update);
        assert_eq!(
            retry_events(&captured.drain()),
            [RetryEvent::Main(8), RetryEvent::Input(105)],
        );
    }
}
