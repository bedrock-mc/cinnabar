//! Physical mouse presses reach actor transactions through the production input adapter.

use super::*;
use bevy::{
    input::{
        ButtonState,
        mouse::{AccumulatedMouseMotion, MouseButtonInput, mouse_button_input_system},
        touch::Touches,
    },
    window::{CursorGrabMode, CursorOptions, WindowEvent},
};
use protocol::test_support::transactions::observe_transaction;

/// Supplies a published actor pick and the physical resources used by mouse sampling.
fn mouse_fixture() -> (World, client_session::CapturedPackets) {
    let (mut world, captured) = attack_fixture(7.5);
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(&world)
        .unwrap();
    world.entity_mut(window).insert(CursorOptions {
        grab_mode: CursorGrabMode::Locked,
        visible: false,
        ..Default::default()
    });
    world.init_resource::<ButtonInput<KeyCode>>();
    world.init_resource::<ButtonInput<MouseButton>>();
    world.init_resource::<Messages<MouseButtonInput>>();
    world.init_resource::<Messages<WindowEvent>>();
    world.init_resource::<AccumulatedMouseMotion>();
    world.init_resource::<Touches>();
    world.init_resource::<crate::semantic_controls::SemanticTouchTargets>();
    world.init_resource::<crate::semantic_controls::PendingDeviceFrame>();
    world.init_resource::<crate::semantic_controls::SemanticRouteState>();
    (world, captured)
}

/// Samples all transitions together, finalizes UI authority, then admits one rendered frame.
fn mouse_frame(world: &mut World, button: MouseButton, states: &[ButtonState]) {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .unwrap();
    for state in states {
        let event = MouseButtonInput {
            button,
            state: *state,
            window,
        };
        world
            .resource_mut::<Messages<MouseButtonInput>>()
            .write(event);
        world
            .resource_mut::<Messages<WindowEvent>>()
            .write(WindowEvent::MouseButtonInput(event));
    }
    world.run_system_cached(mouse_button_input_system).unwrap();
    world
        .run_system_cached(crate::semantic_controls::collect_raw_input)
        .unwrap();
    world
        .run_system_cached(crate::semantic_controls::route_semantic_input)
        .unwrap();
    world
        .run_system_cached(crate::semantic_controls::finalize_semantic_input_after_ui_authority)
        .unwrap();
    world
        .run_system_cached(crate::melee::prepare_melee)
        .unwrap();
    world
        .run_system_cached(crate::melee::produce_attacks)
        .unwrap();
    world
        .run_system_cached(crate::item_use::produce_item_use)
        .unwrap();
    world.resource_mut::<Messages<MouseButtonInput>>().clear();
    world.resource_mut::<Messages<WindowEvent>>().clear();
}

/// Counts admitted actor attacks, checking that every transaction kept the selected actor.
fn attack_count(captured: &mut client_session::CapturedPackets) -> usize {
    captured
        .drain()
        .iter()
        .filter_map(observe_transaction)
        .filter_map(|transaction| transaction.actor_use().cloned())
        .filter(|actor| actor.action == Some(protocol::ActorUseAction::Attack))
        .inspect(|actor| assert_eq!(actor.runtime_id, 99))
        .count()
}

#[test]
fn ordered_mouse_separate_taps_each_send_once_without_tick_advancement() {
    let (mut world, mut captured) = mouse_fixture();
    for _ in 0..2 {
        mouse_frame(
            &mut world,
            MouseButton::Left,
            &[ButtonState::Pressed, ButtonState::Released],
        );
        assert_eq!(attack_count(&mut captured), 1);
        mouse_frame(&mut world, MouseButton::Left, &[]);
        assert_eq!(attack_count(&mut captured), 0, "a tap must not replay");
    }
}

#[test]
fn ordered_mouse_two_taps_in_one_frame_each_send_an_actor_attack() {
    let (mut world, mut captured) = mouse_fixture();
    mouse_frame(
        &mut world,
        MouseButton::Left,
        &[
            ButtonState::Pressed,
            ButtonState::Released,
            ButtonState::Pressed,
            ButtonState::Released,
        ],
    );
    assert_eq!(attack_count(&mut captured), 2);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert_eq!(
        attack_count(&mut captured),
        0,
        "drained taps must not replay"
    );
}

#[test]
fn ordered_mouse_a_repress_in_one_frame_sends_once_per_press_then_stays_held() {
    let (mut world, mut captured) = mouse_fixture();
    mouse_frame(
        &mut world,
        MouseButton::Left,
        &[
            ButtonState::Pressed,
            ButtonState::Released,
            ButtonState::Pressed,
        ],
    );
    assert_eq!(attack_count(&mut captured), 2);
    assert!(
        world
            .resource::<SemanticInputSnapshot>()
            .phase(Action::Attack)
            .held
    );
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert_eq!(attack_count(&mut captured), 0, "holding must not re-attack");
    mouse_frame(&mut world, MouseButton::Left, &[ButtonState::Released]);
    assert_eq!(attack_count(&mut captured), 0, "release must not attack");
}

#[test]
fn ordered_mouse_repeated_presses_follow_the_remapped_attack_button() {
    use launcher::menu::settings_options::{KEY_BINDINGS, SettingsOptions};

    let (mut world, mut captured) = mouse_fixture();
    let mut settings = SettingsOptions::default();
    let row = KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == Action::Attack)
        .unwrap();
    assert!(settings.remap(row, semantic_input::PhysicalControl::MouseButton(4)));
    world
        .resource_mut::<crate::semantic_controls::SemanticInputRuntime>()
        .replace_bindings(settings.controls().unwrap())
        .unwrap();
    mouse_frame(&mut world, MouseButton::Back, &[]);
    assert_eq!(attack_count(&mut captured), 0);
    mouse_frame(
        &mut world,
        MouseButton::Left,
        &[ButtonState::Pressed, ButtonState::Released],
    );
    assert_eq!(attack_count(&mut captured), 0, "the old binding is retired");
    mouse_frame(
        &mut world,
        MouseButton::Back,
        &[
            ButtonState::Pressed,
            ButtonState::Released,
            ButtonState::Pressed,
            ButtonState::Released,
        ],
    );
    assert_eq!(attack_count(&mut captured), 2);
    mouse_frame(&mut world, MouseButton::Back, &[]);
    assert_eq!(
        attack_count(&mut captured),
        0,
        "remapped taps must not replay"
    );
}

#[test]
fn ordered_mouse_unfocused_taps_do_not_replay_after_focus_returns() {
    let (mut world, mut captured) = mouse_fixture();
    world
        .query::<&mut Window>()
        .single_mut(&mut world)
        .unwrap()
        .focused = false;
    mouse_frame(
        &mut world,
        MouseButton::Left,
        &[
            ButtonState::Pressed,
            ButtonState::Released,
            ButtonState::Pressed,
            ButtonState::Released,
        ],
    );
    assert_eq!(attack_count(&mut captured), 0);
    world
        .query::<&mut Window>()
        .single_mut(&mut world)
        .unwrap()
        .focused = true;
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert_eq!(attack_count(&mut captured), 0);
    mouse_frame(
        &mut world,
        MouseButton::Left,
        &[ButtonState::Pressed, ButtonState::Released],
    );
    assert_eq!(attack_count(&mut captured), 1);
}

#[path = "ordered_mouse/ordering.rs"]
mod ordering;
