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
    mouse_frame_after_collection(world, button, states, |_| {});
}

/// Edits a collected batch before routing, including explicit backend-gap fixtures.
fn mouse_frame_after_collection(
    world: &mut World,
    button: MouseButton,
    states: &[ButtonState],
    edit: impl FnOnce(&mut semantic_input::DesktopEvents),
) {
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
        .resource_mut::<crate::semantic_controls::PendingDeviceFrame>()
        .edit_desktop_events(edit);
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
fn ordered_mouse_large_complete_burst_sends_every_actor_attack() {
    let (mut world, _) = mouse_fixture();
    let states: Vec<_> = (0..300)
        .flat_map(|_| [ButtonState::Pressed, ButtonState::Released])
        .collect();
    // Keep packet admission available so this exercises input retention rather than transport pressure.
    let (network, mut captured) =
        NetworkHandle::<PackApplication>::stub_capturing_packets_with_capacity(states.len());
    world.insert_resource(network);
    mouse_frame(&mut world, MouseButton::Left, &states);
    assert_eq!(attack_count(&mut captured), 300);
    mouse_frame(&mut world, MouseButton::Left, &[]);
    assert_eq!(attack_count(&mut captured), 0, "a burst must not replay");
}

/// Delivers movement and consumes its telemetry before the next frame.
fn deliver_movement(world: &mut World) {
    world.resource_scope(|world, mut movement: Mut<MovementTicker>| {
        let network = world.resource::<NetworkHandle<PackApplication>>();
        let mut written = Vec::new();
        gameplay::movement::flush_player_auth_inputs_guarded(
            &mut movement,
            gameplay::movement::OUTBOX_CAPACITY,
            Some(gameplay::test_support::survival_mining::evidence()),
            |identity, packet, guard, _| {
                network.send_physics_packet(identity, packet, guard)?;
                written.push(identity);
                Ok::<(), client_session::PacketSendError>(())
            },
        )
        .unwrap();
        for identity in written {
            assert!(movement.acknowledge_physics_send(identity));
        }
        let _ = movement.take_tick_evidence();
        assert!(movement.physics_is_authorized());
    });
}

#[test]
fn ordered_mouse_25_cps_sends_each_attack_at_60_and_144_fps() {
    for fps in [60, 144] {
        let (mut world, mut captured) = mouse_fixture();
        let mut edge = 0;
        for frame in 0..fps * 4 {
            let first = world.resource::<MovementTicker>().completed_tick() + 1;
            attack_progress_tests::ticks(&mut world, first, 102 + frame * 20 / fps);
            let mut states = Vec::new();
            let mut presses = 0;
            while edge < 200 && edge * fps < (frame + 1) * 50 {
                let pressed = edge % 2 == 0;
                presses += usize::from(pressed);
                states.push(if pressed {
                    ButtonState::Pressed
                } else {
                    ButtonState::Released
                });
                edge += 1;
            }
            mouse_frame(&mut world, MouseButton::Left, &states);
            assert_eq!(
                attack_count(&mut captured),
                presses,
                "every press must send in frame {frame} at {fps} FPS"
            );
            deliver_movement(&mut world);
            let tick = world.resource::<MovementTicker>().completed_tick();
            world
                .resource_mut::<SwingTracker>()
                .published_progress(tick);
            captured.drain();
        }
        assert_eq!(edge, 200);
    }
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
