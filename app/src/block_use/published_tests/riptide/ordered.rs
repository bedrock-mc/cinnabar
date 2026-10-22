//! An observed charged release keeps its gameplay-owned place before later edges.
use super::*;
use client_interaction::item_use::ItemUseRuntime;

#[path = "ordered/matrix.rs"]
mod matrix;

#[path = "ordered/correction.rs"]
mod correction;

/// Publishes a release followed by a wheel selection through the semantic desktop adapter.
fn release_and_select(world: &mut World, release: bool) {
    let mut events = semantic_input::DesktopEvents::default();
    if release {
        events.push(semantic_input::DesktopEvent::Button {
            control: semantic_input::PhysicalControl::MouseButton(2),
            pressed: false,
            modifiers: semantic_input::ModifierChord::default(),
        });
    }
    events.push(semantic_input::DesktopEvent::Scroll { next: true });
    ordered_frame(world, events, Vec::new());
}

/// Runs one complete desktop batch through the production ordered pre-physics adapter.
fn ordered_frame(world: &mut World, events: semantic_input::DesktopEvents, mouse_buttons: Vec<u8>) {
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            desktop_events: Some(events),
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(client_state::SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    world.run_system_cached(produce_block_use).unwrap();
}

/// Captures an accepted charge's release before a movement carrier can be staged.
fn observed_release() -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = respawn::charging_world(gameplay::item_use::RIPTIDE_CHARGE_TICKS);
    world.init_resource::<crate::melee::MeleeTarget>();
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        ui.publish_local_runtime_id(&mut player, 7, 42).unwrap();
    });
    set_hotbar_stack(&mut world, 1, None);
    set_hotbar_stack(&mut world, 2, None);
    let selection = client_interaction::block_use::verified_use_selection(
        world.resource::<client_state::PlayerRuntime>(),
        world.resource::<UiRuntimeResource>(),
    )
    .unwrap();
    let edge = world
        .resource::<MovementTicker>()
        .between_ticks_sample()
        .unwrap();
    assert!(
        world
            .resource_mut::<ItemUseRuntime>()
            .observe_riptide_release(
                edge.tick,
                false,
                Some(gameplay::item_use::RiptideUse {
                    level: 3,
                    wet: true,
                    mounted: false,
                    usable: true
                }),
                Some(&selection),
                gameplay::item_use::RiptideFacing {
                    rotation: [0.0, 90.0],
                    assisted: true
                },
            )
    );
    assert!(!world.resource::<MovementTicker>().has_held_release());
    (world, captured)
}

#[test]
fn ordered_riptide_observed_release_precedes_later_selection() {
    let (mut world, mut captured) = observed_release();
    release_and_select(&mut world, true);
    assert_eq!(
        world
            .resource::<client_state::PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(0),
        "a later selection must not retire an observed release waiting for its facing tick"
    );
    assert!(world.resource::<ItemUseRuntime>().is_using());
    assert!(captured.drain().is_empty());

    tick(&mut world);
    world
        .run_system_cached(crate::item_use::prepare_item_release)
        .unwrap();
    assert!(world.resource::<MovementTicker>().has_held_release());
    flush_and_acknowledge(&mut world);
    let input = captured.drain();
    assert_eq!(input.len(), 1);
    let facing = protocol::player_auth_input_trace_sample(&input[0]).unwrap();
    assert!((facing.yaw - 90.0).abs() < 1e-3);
    world
        .run_system_cached(crate::item_use::produce_item_use)
        .unwrap();
    let release = captured.drain();
    assert_eq!(release.len(), 2);
    assert!(
        protocol::test_support::transactions::observe_transaction(&release[0])
            .unwrap()
            .release()
            .is_some()
    );
    assert_eq!(
        protocol::test_support::packets::equipment(&release[1])
            .unwrap()
            .selected_slot,
        1,
        "the deferred selection may follow its release in the same frame"
    );
    assert!(tick(&mut world).processed.spin_started);
    flush_and_acknowledge(&mut world);
    captured.drain();
    release_and_select(&mut world, false);
    assert_eq!(
        world
            .resource::<client_state::PlayerRuntime>()
            .selected_hotbar_slot(),
        Some(2)
    );
    assert_eq!(
        captured
            .drain()
            .iter()
            .filter_map(protocol::test_support::packets::equipment)
            .count(),
        1
    );
}

#[test]
fn ordered_riptide_observed_release_precedes_fresh_held_mining() {
    let (mut world, mut captured) = observed_release();
    let mut events = semantic_input::DesktopEvents::default();
    for (button, pressed) in [(2, false), (1, true)] {
        events.push(semantic_input::DesktopEvent::Button {
            control: semantic_input::PhysicalControl::MouseButton(button),
            pressed,
            modifiers: semantic_input::ModifierChord::default(),
        });
    }
    ordered_frame(&mut world, events, vec![1]);
    tick(&mut world);
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
    world
        .run_system_cached(crate::item_use::prepare_item_release)
        .unwrap();
    flush_and_acknowledge(&mut world);
    let facing = captured.drain();
    assert!(
        facing
            .iter()
            .flat_map(protocol::test_support::packets::input_block_actions)
            .all(|action| action.0 != protocol::BlockActionKind::StartDestroy.wire_value()),
        "fresh held mining must not overtake the accepted charged release"
    );
    world
        .run_system_cached(crate::item_use::produce_item_use)
        .unwrap();
    let release = captured.drain();
    assert_eq!(
        release
            .iter()
            .filter_map(protocol::test_support::transactions::observe_transaction)
            .filter(|tx| tx.release().is_some())
            .count(),
        1
    );
    tick(&mut world);
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
    flush_and_acknowledge(&mut world);
    assert!(
        captured
            .drain()
            .iter()
            .flat_map(protocol::test_support::packets::input_block_actions)
            .any(|action| action.0 == protocol::BlockActionKind::StartDestroy.wire_value()),
        "the current held input can start mining after release completion"
    );
}

/// A pending server clear waits for a Use release that sits behind an earlier
/// ordered edge in the same ticking frame, so the release still sends its
/// transaction, as vanilla handles input before its movement tick. A re-press
/// after the release does not hide it.
#[test]
fn ordered_use_release_behind_a_pick_precedes_a_server_clear() {
    for represses in [false, true] {
        ordered_use_release_behind_a_pick(represses);
    }
}

fn ordered_use_release_behind_a_pick(represses: bool) {
    let (mut world, mut captured) =
        respawn::charging_world(gameplay::item_use::RIPTIDE_CHARGE_TICKS);
    world.init_resource::<crate::melee::MeleeTarget>();
    world.init_resource::<client_state::GameplayScreenControls>();
    let clear = client_world::MovementFlagUpdate {
        using_item: Some(false),
        ..Default::default()
    };
    world
        .resource_mut::<LocalPhysicsController>()
        .apply_server_movement_flags(0, clear);
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION);
    let mut events = semantic_input::DesktopEvents::default();
    let mut edges = vec![(3, true), (3, false), (2, false)];
    if represses {
        edges.push((2, true));
    }
    for (button, pressed) in edges {
        events.push(semantic_input::DesktopEvent::Button {
            control: semantic_input::PhysicalControl::MouseButton(button),
            pressed,
            modifiers: semantic_input::ModifierChord::default(),
        });
    }
    // A re-press leaves the button down when the frame's input ends.
    let held_at_end = if represses { vec![2] } else { Vec::new() };
    ordered_frame(&mut world, events, held_at_end);
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    tick(&mut world);
    respawn::item_phases(&mut world);
    // The charged release waits for the input carrying its facing.
    let mut packets = captured.drain();
    tick(&mut world);
    respawn::item_phases(&mut world);
    packets.extend(captured.drain());
    let releases = packets
        .iter()
        .filter_map(protocol::test_support::transactions::observe_transaction)
        .filter(|transaction| transaction.release().is_some())
        .count();
    assert_eq!(releases, 1, "the queued Use release keeps its transaction");
}

/// A spectator game-mode stop reaches item use before later ordered input in
/// the frame, so a Use release after it has no use left to release, as
/// vanilla stops the use while handling the packet.
#[test]
fn a_game_mode_stop_precedes_later_ordered_use_input() {
    let (mut world, mut captured) =
        respawn::charging_world(gameplay::item_use::RIPTIDE_CHARGE_TICKS);
    world.init_resource::<crate::melee::MeleeTarget>();
    world.init_resource::<client_state::GameplayScreenControls>();
    world
        .resource_mut::<LocalPhysicsController>()
        .stop_item_use_for_game_mode();
    let mut events = semantic_input::DesktopEvents::default();
    events.push(semantic_input::DesktopEvent::Button {
        control: semantic_input::PhysicalControl::MouseButton(2),
        pressed: false,
        modifiers: semantic_input::ModifierChord::default(),
    });
    ordered_frame(&mut world, events, Vec::new());
    assert!(!world.resource::<ItemUseRuntime>().is_using());
    assert!(!world.resource::<MovementTicker>().has_held_release());
    let releases = captured
        .drain()
        .iter()
        .filter_map(protocol::test_support::transactions::observe_transaction)
        .filter(|transaction| transaction.release().is_some())
        .count();
    assert_eq!(releases, 0);
}
