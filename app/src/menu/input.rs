use std::{fs, path::PathBuf};

use bevy::{
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
        touch::Touches,
    },
    prelude::{
        AppExit, ButtonInput, KeyCode, MessageReader, MessageWriter, MouseButton, Query, Res,
        ResMut, Single, With,
    },
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use ui::UiPoint;

use crate::{runtime::network::NetworkHandle, ui_runtime::presentation::UiPresentationRuntime};

use super::{CoreProcessGuard, MenuRuntime, MenuScreen, spawn_core_for_address, wait_for_core};

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_menu_input(
    mut keyboard_messages: MessageReader<KeyboardInput>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    gamepads: Query<&Gamepad>,
    presentation: Res<UiPresentationRuntime>,
    mut menu: ResMut<MenuRuntime>,
) {
    let (window, mut cursor) = window.into_inner();
    menu.pressed = None;
    if !window.focused {
        menu.pointer_down = false;
        return;
    }
    if !menu.is_visible() {
        menu.hovered = None;
        menu.pointer_down = false;
        if keys.just_pressed(KeyCode::Escape) {
            menu.open_pause();
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
            keys.reset_all();
        }
        return;
    }

    cursor.grab_mode = CursorGrabMode::None;
    cursor.visible = true;
    menu.hovered = window
        .cursor_position()
        .and_then(|position| UiPoint::new(position.x, position.y).ok())
        .and_then(|position| presentation.hit_test_menu(position));
    let pointer_pressed = mouse_buttons.pressed(MouseButton::Left);
    let pointer_just_pressed = pointer_pressed && !menu.pointer_down;
    menu.pointer_down = pointer_pressed;
    if pointer_just_pressed && let Some(action) = menu.hovered {
        menu.activate(action);
    }
    for touch in touches.iter_just_pressed() {
        let position = touch.position();
        if let Ok(position) = UiPoint::new(position.x, position.y)
            && let Some(action) = presentation.hit_test_menu(position)
        {
            menu.activate(action);
        }
    }
    for gamepad in &gamepads {
        if gamepad.just_pressed(GamepadButton::DPadUp)
            || gamepad.just_pressed(GamepadButton::DPadLeft)
        {
            menu.move_focus(-1);
        }
        if gamepad.just_pressed(GamepadButton::DPadDown)
            || gamepad.just_pressed(GamepadButton::DPadRight)
        {
            menu.move_focus(1);
        }
        if gamepad.just_pressed(GamepadButton::South) {
            menu.activate_focused();
        }
        if gamepad.just_pressed(GamepadButton::East) {
            menu.go_back_from_input();
        }
    }
    for input in keyboard_messages.read() {
        if input.state != ButtonState::Pressed {
            continue;
        }
        match input.key_code {
            KeyCode::Escape => menu.go_back_from_input(),
            KeyCode::ArrowUp | KeyCode::ArrowLeft => menu.move_focus(-1),
            KeyCode::ArrowDown | KeyCode::ArrowRight | KeyCode::Tab => menu.move_focus(1),
            KeyCode::Enter | KeyCode::NumpadEnter => menu.activate_focused(),
            KeyCode::Backspace if menu.field.is_some() => menu.backspace_text(),
            _ if menu.field.is_some() => {
                if let Some(text) = input.text.as_deref() {
                    menu.edit_text(text);
                }
            }
            _ => {}
        }
    }
    // The menu owns the pointer and keyboard for this frame. This also keeps
    // the camera's recapture-on-click path from turning a menu click into a
    // gameplay attack or mouse grab.
    keys.reset_all();
    mouse_buttons.reset_all();
}

impl MenuRuntime {
    fn go_back_from_input(&mut self) {
        self.go_back();
    }
}

pub(crate) fn drive_menu_connection(
    mut commands: bevy::prelude::Commands,
    mut exits: MessageWriter<AppExit>,
    mut menu: ResMut<MenuRuntime>,
    mut guard: ResMut<CoreProcessGuard>,
    mut network: ResMut<NetworkHandle>,
    mut runtime: ResMut<crate::ui_runtime::UiRuntime>,
    mut client_world: ResMut<crate::runtime::world::ClientWorld>,
) {
    menu.poll_catalog();
    if menu.is_connecting() && client_world.stream.is_some() {
        menu.mark_connected();
    }
    if let Some(pending) = menu.take_pending_connect() {
        let address = pending.address;
        let generation = menu.next_session_generation();
        // Namespaced by process id like the `--address` path: a bare
        // generation counter restarts at the same value every launch, so a
        // previous run's directory would be reused for this session.
        let socket_dir = PathBuf::from(format!(
            ".local/cinnabar/connect-{}-{generation}",
            std::process::id()
        ));
        if let Err(error) = fs::create_dir_all(&socket_dir)
            .and_then(|_| {
                spawn_core_for_address(&socket_dir, &address, pending.auth_cache.as_deref())
                    .map_err(std::io::Error::other)
            })
            .and_then(|child| {
                guard.replace(child);
                wait_for_core(&socket_dir).map_err(std::io::Error::other)
            })
        {
            menu.message = Some(format!("Could not start {address}: {error}"));
            menu.connecting = false;
            return;
        }
        network.shutdown();
        match crate::runtime::network::spawn_network(crate::runtime::network::NetworkConfig {
            session_generation: generation,
            socket_dir,
            display_name: menu.display_name.clone(),
            client_blob_cache: protocol::ClientBlobCache::default(),
        }) {
            Ok(replacement) => {
                runtime.begin_session(generation);
                client_world.stream = None;
                client_world.pending_surface_spawn = None;
                client_world.fatal_error = None;
                commands.insert_resource(replacement.movement_ticker());
                commands.insert_resource(replacement);
                menu.mark_connecting();
            }
            Err(error) => {
                menu.message = Some(format!("Could not connect: {error}"));
                menu.connecting = false;
            }
        }
    }
    if menu.take_disconnect_request() {
        network.shutdown();
        guard.stop();
        runtime.begin_session(menu.next_session_generation());
        client_world.stream = None;
        menu.visible = true;
        menu.screen = MenuScreen::Home;
        menu.connecting = false;
    }
    if menu.take_exit_request() {
        network.shutdown();
        guard.stop();
        exits.write(AppExit::Success);
    }
}

/// Returns a failed launcher session to the menu instead of ending the process.
///
/// Runs late in the frame: the failure is recorded while network events are
/// drained, which is after the menu's own systems, so recovery has to happen
/// between that and the systems that exit on a fatal error.
pub(crate) fn recover_menu_session_failure(
    mut menu: ResMut<MenuRuntime>,
    mut guard: ResMut<CoreProcessGuard>,
    mut network: ResMut<NetworkHandle>,
    mut runtime: ResMut<crate::ui_runtime::UiRuntime>,
    mut client_world: ResMut<crate::runtime::world::ClientWorld>,
) {
    let Some(error) = client_world.fatal_error.clone() else {
        return;
    };
    if !menu.absorb_session_failure(&error) {
        return;
    }
    network.shutdown();
    guard.stop();
    runtime.begin_session(menu.next_session_generation());
    client_world.stream = None;
    client_world.pending_surface_spawn = None;
    client_world.fatal_error = None;
}
