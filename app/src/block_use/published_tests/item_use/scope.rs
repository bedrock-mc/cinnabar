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
