//! A new world's server use survives the first ordered input hand-off.

use super::*;
use client_interaction::{item_use::ItemUseRuntime, movement::LocalPhysicsController};

/// Delivers a new-world server start and a Use release while item use still owns the old scope.
fn server_start_after_scope_change(previous_session: u64, previous_dimension: i32) {
    let (mut world, mut captured) = fixture();
    hold_air_item(&mut world, "minecraft:bow", 1, [].into());
    world.init_resource::<crate::melee::MeleeTarget>();
    {
        let mut runtime = world.resource_mut::<ItemUseRuntime>();
        runtime.synchronize(previous_session);
        runtime.observe_world(SessionGeneration::new(previous_session), previous_dimension);
    }
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.5, 2.620_01, 8.5], 101, false);
    physics.apply_server_movement_flags(
        0,
        client_world::MovementFlagUpdate {
            using_item: Some(true),
            ..Default::default()
        },
    );
    world.insert_resource(physics);
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION);
    let mut events = semantic_input::DesktopEvents::default();
    events.push(semantic_input::DesktopEvent::Button {
        control: semantic_input::PhysicalControl::MouseButton(2),
        pressed: false,
        modifiers: Default::default(),
    });
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            desktop_events: Some(events),
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame::default()),
            ..Default::default()
        })
        .unwrap();
    assert!(snapshot.phases[Action::Use as usize].released);
    assert_eq!(snapshot.ordered.as_ref().unwrap().events().len(), 1);
    world.insert_resource(SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
    world.run_system_cached(produce_block_use).unwrap();
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    assert!(
        world.resource::<ItemUseRuntime>().is_using(),
        "the new world's server start must survive the ordered Use edge"
    );
    assert_eq!(
        world
            .resource::<ItemUseRuntime>()
            .active_timing()
            .unwrap()
            .0,
        world.resource::<MovementTicker>().next_tick()
    );
    assert_eq!(
        world
            .resource_mut::<LocalPhysicsController>()
            .take_server_using_item(),
        None
    );
    assert!(
        captured.drain().iter().all(|packet| {
            protocol::test_support::transactions::observe_transaction(packet).is_none()
        }),
        "the server start needs no local use or release transaction"
    );
}

#[test]
fn server_item_use_survives_an_ordered_edge_after_a_session_change() {
    server_start_after_scope_change(6, 0);
}

#[test]
fn server_item_use_survives_an_ordered_edge_after_a_dimension_change() {
    server_start_after_scope_change(7, 1);
}

/// Starts a server-owned bow use with two verified slots and the requested button ownership.
fn server_using_bow(held: bool) -> (World, client_session::CapturedPackets) {
    let (mut world, mut captured) = fixture();
    hold_air_item(&mut world, "minecraft:bow", 1, [].into());
    world.init_resource::<crate::melee::MeleeTarget>();
    world.init_resource::<client_state::GameplayScreenControls>();
    world.resource_scope(|world, mut ui: Mut<UiRuntimeResource>| {
        let mut player = world.resource_mut::<client_state::PlayerRuntime>();
        ui.publish_local_runtime_id(&mut player, 7, 42).unwrap();
        let ledger = ui.inventory_ledger_mut(&mut player);
        let mut stack = ledger.backing_stack(0).unwrap().clone();
        stack.stack_network_id += 1;
        ledger.apply(&InventoryEvent::Slot(InventorySlotEvent {
            identity: SlotIdentity {
                container: ContainerIdentity {
                    window_id: Some(0),
                    slot_type: None,
                    dynamic_id: None,
                },
                slot: 1,
            },
            stack,
            storage_item: None,
        }));
    });
    world
        .run_system_cached(crate::item_use::ordered::synchronize_scope)
        .unwrap();
    let using = client_world::MovementFlagUpdate {
        using_item: Some(true),
        ..Default::default()
    };
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.5, 2.620_01, 8.5], 101, false);
    physics.apply_server_movement_flags(0, using);
    world.insert_resource(physics);
    world
        .resource_mut::<Time<Real>>()
        .advance_by(simulation_clock::TICK_DURATION);
    if !held {
        route_use_events(
            &mut world,
            semantic_input::DesktopEvents::default(),
            Vec::new(),
        );
    }
    world
        .run_system_cached(crate::item_use::adopt_server_item_use)
        .unwrap();
    assert!(world.resource::<ItemUseRuntime>().is_using());
    captured.drain();
    (world, captured)
}

/// Finalizes one ordered batch against the fixture's current button state.
fn route_use_events(world: &mut World, events: semantic_input::DesktopEvents, buttons: Vec<u8>) {
    let snapshot = world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .route_and_finalize(semantic_input::DeviceFrame {
            desktop_events: Some(events),
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                mouse_buttons: buttons,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    world.insert_resource(SemanticInputSnapshot(
        semantic_input::finalized::FinalizedInput::from_finalized(snapshot),
    ));
}

/// A slot change clears an active use, while an earlier game-mode stop preserves a later start.
#[test]
fn a_local_reselection_orders_game_mode_stops_and_pending_server_starts() {
    for game_mode_stop in [false, true] {
        let (mut world, mut captured) = server_using_bow(true);
        {
            let mut physics = world.resource_mut::<LocalPhysicsController>();
            if game_mode_stop {
                physics.stop_item_use_for_game_mode();
            }
            physics.apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    using_item: Some(true),
                    ..Default::default()
                },
            );
        }
        let mut events = semantic_input::DesktopEvents::default();
        events.push(semantic_input::DesktopEvent::Scroll { next: true });
        events.push(semantic_input::DesktopEvent::Button {
            control: semantic_input::PhysicalControl::MouseButton(2),
            pressed: false,
            modifiers: Default::default(),
        });
        route_use_events(&mut world, events, Vec::new());
        world.run_system_cached(produce_block_use).unwrap();
        assert_eq!(
            world
                .resource::<client_state::PlayerRuntime>()
                .selected_hotbar_slot(),
            Some(1)
        );
        world
            .run_system_cached(crate::item_use::adopt_server_item_use)
            .unwrap();
        let runtime = world.resource::<ItemUseRuntime>();
        assert_eq!(
            runtime.is_using(),
            game_mode_stop,
            "the latest item-use transition wins"
        );
        assert_eq!(runtime.movement_modifier().is_some(), game_mode_stop);
        assert!(!runtime.has_server_using_item());
        assert_eq!(
            world.resource::<LocalPhysicsController>().live_using_item(),
            game_mode_stop
        );
        assert!(captured.drain().iter().all(|packet| {
            protocol::test_support::transactions::observe_transaction(packet).is_none()
        }));
    }
}

/// Deferred button transitions preserve a real release without inventing one for a held press.
#[test]
fn deferred_use_edges_keep_server_clears_in_input_order() {
    for buttons in [vec![true], vec![true, false], vec![false, true]] {
        let releasing = buttons == [true, false];
        let (mut world, mut captured) = server_using_bow(false);
        world
            .resource_mut::<LocalPhysicsController>()
            .apply_server_movement_flags(
                0,
                client_world::MovementFlagUpdate {
                    using_item: Some(false),
                    ..Default::default()
                },
            );
        let mut events = semantic_input::DesktopEvents::default();
        for (button, pressed) in [(3, true), (3, false)] {
            events.push(semantic_input::DesktopEvent::Button {
                control: semantic_input::PhysicalControl::MouseButton(button),
                pressed,
                modifiers: Default::default(),
            });
        }
        for pressed in &buttons {
            events.push(semantic_input::DesktopEvent::Button {
                control: semantic_input::PhysicalControl::MouseButton(2),
                pressed: *pressed,
                modifiers: Default::default(),
            });
        }
        let held = if *buttons.last().unwrap() {
            vec![2]
        } else {
            Vec::new()
        };
        route_use_events(&mut world, events, held);
        world.run_system_cached(produce_block_use).unwrap();
        world
            .run_system_cached(crate::item_use::adopt_server_item_use)
            .unwrap();
        assert_eq!(
            world.resource::<ItemUseRuntime>().is_using(),
            releasing,
            "only a real owned release may precede the server clear: {buttons:?}"
        );
        crate::ordered_interactions::drain(
            &mut world,
            crate::ordered_interactions::DispatchPhase::AfterMovement,
        );
        assert!(!world.resource::<ItemUseRuntime>().is_using());
        let transactions: Vec<_> = captured
            .drain()
            .iter()
            .filter_map(protocol::test_support::transactions::observe_transaction)
            .collect();
        assert_eq!(
            transactions
                .iter()
                .filter(|transaction| transaction.release().is_some())
                .count(),
            usize::from(releasing)
        );
        assert!(
            !transactions
                .iter()
                .any(|transaction| transaction.item_use().is_some())
        );
    }
}
