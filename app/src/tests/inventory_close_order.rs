//! Same-frame pointer and close-key ordering through the production input owners.

use crate::player_runtime::PlayerRuntime;
use std::sync::Arc;

use bevy::{
    input::{
        ButtonInput, ButtonState,
        keyboard::{Key, KeyCode, KeyboardInput},
        mouse::{AccumulatedMouseMotion, MouseButtonInput},
        touch::Touches,
    },
    prelude::{App, Entity, IntoScheduleConfigs, MouseButton, Update},
    time::{Real, Time},
    window::{CursorOptions, PrimaryWindow, Window, WindowEvent},
};
use protocol::{
    ContainerIdentity, ContainerOpenEvent, InventoryAuthority, InventoryContentEvent,
    InventoryEvent, NetworkItemStack,
};
use ui::UiPoint;

use crate::{
    app::{ClientFrameSet, configure_client_frame_schedule},
    menu::{MenuClipboard, MenuRuntime, drive_menu_input},
    ui_runtime::{
        drive_chat_keyboard_input, drive_inventory_ui_actions, presentation::tests::fixture_font,
    },
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

fn app() -> (App, Entity) {
    app_with(true)
}

/// One stack in slot zero under the pointer, with the personal inventory open or closed.
fn app_with(open: bool) -> (App, Entity) {
    let mut player_runtime = PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.publish_inventory_authority(&mut player_runtime, InventoryAuthority::Server);
    runtime
        .publish_local_runtime_id(&mut player_runtime, 1, 42)
        .unwrap();
    runtime
        .inventory_ledger_mut(&mut player_runtime)
        .apply(&InventoryEvent::Content(InventoryContentEvent {
            container: ContainerIdentity::window(0),
            slots: Arc::from(
                (0..36)
                    .map(|index| NetworkItemStack {
                        network_id: if index == 0 { 6 } else { 0 },
                        count: if index == 0 { 2 } else { 0 },
                        stack_network_id: if index == 0 { 55 } else { 0 },
                        ..NetworkItemStack::default()
                    })
                    .collect::<Vec<_>>(),
            ),
            storage_item: NetworkItemStack::default(),
        }));
    if open {
        runtime.toggle_inventory(&mut player_runtime);
        assert!(
            runtime
                .inventory_ledger_mut(&mut player_runtime)
                .mark_transport_enqueued(0)
        );
        runtime
            .inventory_ledger_mut(&mut player_runtime)
            .apply(&InventoryEvent::Open(ContainerOpenEvent {
                container: ContainerIdentity::window(2),
                window_type: -1,
                position: [0, 64, 0],
                runtime_entity_id: -1,
            }));
    }

    let presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
    let physical_size = [1280, 720];
    let pointer = (0..physical_size[1])
        .find_map(|y| {
            (0..physical_size[0]).find_map(|x| {
                let point = UiPoint::new(x as f32, y as f32).unwrap();
                let gui = presentation.inventory_gui_point(point, physical_size, 1.0)?;
                (presentation.inventory_slot_hit(gui, physical_size, 1.0) == Some(0))
                    .then_some(bevy::math::Vec2::new(x as f32, y as f32))
            })
        })
        .expect("slot zero has a physical hit point");
    let mut window = Window {
        focused: true,
        resolution: bevy::window::WindowResolution::new(1280, 720),
        ..Default::default()
    };
    window.set_cursor_position(Some(pointer));

    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .add_message::<KeyboardInput>()
        // As in frames without a fixed tick, window events are not rotated between updates.
        .init_resource::<bevy::ecs::message::Messages<WindowEvent>>()
        .insert_resource(runtime)
        .insert_resource(player_runtime)
        .insert_resource(presentation)
        .insert_resource(MenuRuntime::new(false, 2, "Tester".to_owned()))
        .add_systems(
            Update,
            (
                drive_chat_keyboard_input,
                drive_menu_input,
                drive_inventory_ui_actions,
            )
                .chain()
                .in_set(ClientFrameSet::UiAuthority),
        );
    let window = app
        .world_mut()
        .spawn((window, CursorOptions::default(), PrimaryWindow))
        .id();
    (app, window)
}

#[derive(Clone, Copy)]
enum Input {
    Click(MouseButton),
    Key(KeyCode),
}

/// Delivers one frame's inputs as winit does: ordered window events plus the per-device streams.
fn frame(app: &mut App, window: Entity, inputs: &[Input]) {
    for input in inputs {
        let event = match *input {
            Input::Click(button) => {
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .press(button);
                WindowEvent::MouseButtonInput(MouseButtonInput {
                    button,
                    state: ButtonState::Pressed,
                    window,
                })
            }
            Input::Key(key_code) => {
                app.world_mut()
                    .resource_mut::<ButtonInput<KeyCode>>()
                    .press(key_code);
                let (logical_key, text) = match key_code {
                    KeyCode::Escape => (Key::Escape, None),
                    KeyCode::KeyT => (Key::Character("t".into()), Some("t".into())),
                    KeyCode::KeyQ => (Key::Character("q".into()), Some("q".into())),
                    KeyCode::KeyE => (Key::Character("e".into()), Some("e".into())),
                    KeyCode::KeyU => (Key::Character("u".into()), Some("u".into())),
                    other => panic!("unmapped test key {other:?}"),
                };
                let input = KeyboardInput {
                    key_code,
                    logical_key,
                    state: ButtonState::Pressed,
                    text,
                    repeat: false,
                    window,
                };
                app.world_mut().write_message(input.clone());
                WindowEvent::KeyboardInput(input)
            }
        };
        app.world_mut().write_message(event);
    }
}

fn requests(app: &App) -> usize {
    app.world()
        .resource::<PlayerRuntime>()
        .inventory
        .ledger()
        .pending_request_count()
}

/// A click that reached the window before Escape lands in the screen before it closes.
#[test]
fn click_then_escape_in_one_frame_applies_the_click() {
    let left = Input::Click(MouseButton::Left);
    let escape = Input::Key(KeyCode::Escape);
    for (inputs, applied) in [([left, escape], true), ([escape, left], false)] {
        let (mut app, window) = app();
        frame(&mut app, window, &inputs);

        app.update();

        assert!(!app.world().resource::<UiRuntime>().inventory_open());
        assert_eq!(
            requests(&app) > 0,
            applied,
            "only a click before Escape picks up the stack"
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left),
            "the click never reaches gameplay"
        );
    }
}

/// Keys left over from an earlier frame must not shift where this frame's click arrived.
#[test]
fn earlier_frame_keys_do_not_reorder_a_later_click_and_escape() {
    let (mut app, window) = app();
    let pointer = app.world().get::<Window>(window).unwrap().cursor_position();
    // A click outside the window leaves the screen untouched, then a key follows it.
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(None);
    frame(
        &mut app,
        window,
        &[Input::Click(MouseButton::Left), Input::Key(KeyCode::KeyQ)],
    );
    app.update();
    assert!(app.world().resource::<UiRuntime>().inventory_open());
    assert_eq!(requests(&app), 0);

    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(pointer);
    frame(
        &mut app,
        window,
        &[Input::Click(MouseButton::Left), Input::Key(KeyCode::Escape)],
    );
    app.update();

    assert!(!app.world().resource::<UiRuntime>().inventory_open());
    assert!(
        requests(&app) > 0,
        "the second frame's click lands before Escape"
    );
}

/// Keys after Escape route as if the screen were already closed.
#[test]
fn keys_after_click_and_escape_route_to_the_closed_screen() {
    let take_half = Input::Click(MouseButton::Right);
    let escape = Input::Key(KeyCode::Escape);
    let chat = Input::Key(KeyCode::KeyT);
    let (mut control, window) = app();
    frame(&mut control, window, &[take_half, escape, chat]);
    control.update();
    assert!(requests(&control) > 0, "the click takes half the stack");

    let (mut app, window) = app();
    frame(
        &mut app,
        window,
        &[take_half, escape, Input::Key(KeyCode::KeyQ), chat],
    );
    app.update();

    let runtime = app.world().resource::<UiRuntime>();
    assert!(!runtime.inventory_open());
    assert!(runtime.chat_focused(), "T after Escape opens chat");
    assert_eq!(
        requests(&app),
        requests(&control),
        "Q after Escape cannot drop from the hovered inventory cell"
    );
}

fn queued_batch(app: &App) -> Option<protocol::Packet> {
    app.world()
        .resource::<PlayerRuntime>()
        .inventory
        .ledger()
        .pending_batch()
        .unwrap()
        .map(|(packet, _)| packet)
}

/// Only presses before the close key reach the open screen; a later press cannot override them.
#[test]
fn press_after_escape_cannot_change_the_earlier_take() {
    let take_half = Input::Click(MouseButton::Right);
    let escape = Input::Key(KeyCode::Escape);
    let (mut control, window) = app();
    frame(&mut control, window, &[take_half, escape]);
    control.update();
    assert!(
        queued_batch(&control).is_some(),
        "the right-click takes half"
    );

    let (mut app, window) = app();
    frame(
        &mut app,
        window,
        &[take_half, escape, Input::Click(MouseButton::Left)],
    );
    app.update();

    assert!(!app.world().resource::<UiRuntime>().inventory_open());
    assert_eq!(
        queued_batch(&app),
        queued_batch(&control),
        "the left-click after Escape must not turn the half take into a whole-stack take"
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left),
        "the late click never reaches gameplay"
    );
}

/// A press made before the open key belongs to gameplay, never to the screen it opens.
#[test]
fn press_before_the_open_key_never_reaches_the_opened_screen() {
    let (mut app, window) = app_with(false);
    frame(
        &mut app,
        window,
        &[
            Input::Click(MouseButton::Left),
            Input::Key(KeyCode::KeyE),
            Input::Key(KeyCode::Escape),
        ],
    );
    app.update();

    assert!(!app.world().resource::<UiRuntime>().inventory_open());
    assert_eq!(queued_batch(&app), None, "the inventory receives nothing");
}

/// A press lands once, on the screen showing when it arrived, however often the screen reopens.
#[test]
fn press_is_not_replayed_after_close_and_reopen() {
    let take_half = Input::Click(MouseButton::Right);
    let escape = Input::Key(KeyCode::Escape);
    let (mut control, window) = app();
    frame(&mut control, window, &[take_half, escape]);
    control.update();

    let (mut app, window) = app();
    frame(
        &mut app,
        window,
        &[take_half, escape, Input::Key(KeyCode::KeyE), escape],
    );
    app.update();

    assert_eq!(queued_batch(&app), queued_batch(&control));
}

/// A click that recaptures the cursor keeps its edge for gameplay even when a key follows it.
#[test]
fn gameplay_click_followed_by_a_key_keeps_its_edge() {
    let (mut app, window) = app_with(false);
    frame(
        &mut app,
        window,
        &[Input::Click(MouseButton::Left), Input::Key(KeyCode::KeyQ)],
    );
    app.update();

    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .just_pressed(MouseButton::Left)
    );
}

/// A press that triggers the inventory binding opens the screen and is never also a click on it.
#[test]
fn inventory_binding_press_followed_by_a_key_is_not_a_click() {
    use crate::menu::{
        MenuAction,
        settings_options::{EXTRA_KEYS, KEY_BINDINGS},
    };
    let (mut app, window) = app_with(false);
    let row = |name: &str| {
        (KEY_BINDINGS.iter().position(|(_, key)| *key == name))
            .or_else(|| {
                EXTRA_KEYS
                    .iter()
                    .position(|(key, _)| *key == name)
                    .map(|index| KEY_BINDINGS.len() + index)
            })
            .unwrap() as u16
    };
    // Free the right button from use, then bind the inventory to it through the menu.
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .set_visible(true);
    let rebind = |app: &mut App, name: &str, control: Input| {
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .activate(MenuAction::SettingsKey(row(name)));
        frame(app, window, &[control]);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    };
    rebind(&mut app, "key.use", Input::Key(KeyCode::KeyU));
    rebind(&mut app, "key.inventory", Input::Click(MouseButton::Right));
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        assert_eq!(
            menu.settings_snapshot()
                .0
                .named_key_control("key.inventory"),
            Some(semantic_input::PhysicalControl::MouseButton(
                crate::semantic_controls::physical::mouse_button_code(MouseButton::Right).unwrap()
            )),
            "the inventory is bound to the right button"
        );
        menu.set_visible(false);
    }
    app.update();
    assert!(!app.world().resource::<UiRuntime>().inventory_open());

    frame(
        &mut app,
        window,
        &[Input::Click(MouseButton::Right), Input::Key(KeyCode::KeyQ)],
    );
    app.update();

    assert!(app.world().resource::<UiRuntime>().inventory_open());
    assert_eq!(requests(&app), 0, "no slot click and no drop");
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .just_pressed(MouseButton::Right),
        "the binding consumed its press"
    );
}
