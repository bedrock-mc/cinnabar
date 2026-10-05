use super::{LocalFormAction, engine_focus, engine_input};
use crate::menu::MenuRuntime;
use bevy::{
    ecs::message::{MessageCursor, Messages},
    input::{
        ButtonState,
        gamepad::Gamepad,
        keyboard::KeyboardInput,
        mouse::{AccumulatedMouseMotion, MouseButtonInput, MouseScrollUnit, MouseWheel},
    },
    prelude::{
        ButtonInput, KeyCode, Local, MessageReader, MouseButton, Query, Res, ResMut, Single, Time,
        Window, With,
    },
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(crate) fn drive_server_form_input(
    (player_runtime, window): (
        bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
        Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    ),
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut motion: ResMut<AccumulatedMouseMotion>,
    mut wheel: MessageReader<MouseWheel>,
    mut keyboard: MessageReader<KeyboardInput>,
    button_messages: Option<Res<Messages<MouseButtonInput>>>,
    mut button_cursor: Local<MessageCursor<MouseButtonInput>>,
    mut held: Local<bool>,
    menu: Option<Res<MenuRuntime>>,
    presentation: Res<UiPresentationRuntime>,
    mut runtime: ResMut<UiRuntime>,
    mut owned_last_frame: Local<bool>,
    time: Option<Res<Time<Real>>>,
    pads: Query<&Gamepad>,
    mut stick: Local<[bool; 4]>,
) {
    let (window, mut cursor) = window.into_inner();
    if runtime.credits().owns_input() {
        wheel.clear();
        keyboard.clear();
        *held = false;
        *owned_last_frame = false;
        return;
    }
    // Buttons are reset every owned frame, so a physical release never surfaces
    // as `just_released`; raw messages carry the edges instead.
    let mut pointer = engine_input::PointerButtons {
        pressed: mouse.just_pressed(MouseButton::Left),
        released: mouse.just_released(MouseButton::Left),
        held: false,
    };
    let mut pointer_edges = Vec::new();
    if let Some(messages) = button_messages.as_deref() {
        for input in button_cursor.read(messages) {
            if input.button == MouseButton::Left {
                let down = input.state == ButtonState::Pressed;
                pointer_edges.push(down);
                if down {
                    pointer.pressed = true;
                } else {
                    pointer.released = true;
                }
            }
        }
    }
    if pointer_edges.is_empty() {
        if pointer.pressed {
            pointer_edges.push(true);
        }
        if pointer.released {
            pointer_edges.push(false);
        }
        if pointer_edges.is_empty() {
            *held |= mouse.pressed(MouseButton::Left);
        }
    }
    for down in &pointer_edges {
        *held = *down;
    }
    pointer.held = *held;
    if menu.as_ref().is_some_and(|menu| menu.is_visible())
        && !runtime.server_forms().settings_form_active()
    {
        runtime.server_forms_mut().reject_active_busy();
        wheel.clear();
        keyboard.clear();
        *owned_last_frame = false;
        return;
    }
    if !runtime.server_forms().owns_input() {
        wheel.clear();
        keyboard.clear();
        if *owned_last_frame && !runtime.ui_focused(&player_runtime) && window.focused {
            client_ui::ui_runtime::interaction::restore_gameplay_input_after_chat(
                &mut cursor,
                &mut keys,
                &mut mouse,
                &mut motion,
            );
        }
        *owned_last_frame = false;
        return;
    }
    *owned_last_frame = true;
    let engine_frame = runtime
        .server_forms()
        .active()
        .and_then(|entry| presentation.form_engine_frame(entry.identity))
        .cloned();
    if window.focused
        && let Some(frame) = engine_frame
    {
        let input = engine_input::EngineInput {
            pointer_edges,
            cursor: window
                .cursor_position()
                .and_then(|point| ui::UiPoint::new(point.x, point.y).ok()),
            keys: &keys,
            pointer,
            wheel: wheel.read().map(|event| (event.y, event.unit)).collect(),
            typed: keyboard
                .read()
                .filter(|input| input.state == ButtonState::Pressed)
                .map(|input| {
                    (
                        input.key_code,
                        input.text.as_ref().map(|text| text.to_string()),
                        input.repeat,
                    )
                })
                .chain(
                    engine_focus::gamepad_keys(pads.iter(), &mut stick)
                        .into_iter()
                        .map(|(key, text)| (key, text, false)),
                )
                .collect(),
            now: time.map_or(0.0, |time| time.elapsed_secs_f64()),
            animator: presentation.form_animator(),
        };
        engine_input::drive(&mut runtime, &frame, input);
    } else if window.focused
        && let Some(entry) = runtime.server_forms().active()
    {
        keyboard.clear();
        let identity = entry.identity;
        if keys.just_pressed(KeyCode::ArrowUp)
            || (keys.just_pressed(KeyCode::Tab) && keys.pressed(KeyCode::ShiftLeft))
        {
            runtime.server_forms_mut().move_focus(-1);
        } else if keys.just_pressed(KeyCode::ArrowDown) || keys.just_pressed(KeyCode::Tab) {
            runtime.server_forms_mut().move_focus(1);
        }
        if (keys.just_pressed(KeyCode::ArrowUp)
            || keys.just_pressed(KeyCode::ArrowDown)
            || keys.just_pressed(KeyCode::Tab))
            && let Some(offset) =
                presentation.form_focus_scroll(identity, runtime.server_forms().focus())
        {
            runtime.server_forms_mut().set_scroll(offset);
        }
        for event in wheel.read() {
            let pixels = match event.unit {
                MouseScrollUnit::Line => event.y * 44.0,
                MouseScrollUnit::Pixel => event.y,
            };
            runtime
                .server_forms_mut()
                .scroll_rows((-pixels).clamp(-4096.0, 4096.0) as i32);
        }
        let action = if keys.just_pressed(KeyCode::Escape) {
            Some((identity, LocalFormAction::Dismiss))
        } else if keys.just_pressed(KeyCode::Enter)
            || keys.just_pressed(KeyCode::NumpadEnter)
            || keys.just_pressed(KeyCode::Space)
        {
            presentation.form_button_count(identity).and_then(|count| {
                let index = runtime.server_forms().focus();
                if index < count && !presentation.form_button_visible(identity, index) {
                    if let Some(offset) = presentation.form_focus_scroll(identity, index) {
                        runtime.server_forms_mut().set_scroll(offset);
                    }
                    return None;
                }
                Some((
                    identity,
                    if index < count {
                        LocalFormAction::SubmitButton(index as u32)
                    } else {
                        LocalFormAction::Dismiss
                    },
                ))
            })
        } else if pointer.pressed {
            window
                .cursor_position()
                .and_then(|point| ui::UiPoint::new(point.x, point.y).ok())
                .and_then(|point| presentation.hit_test_form(point))
        } else {
            None
        };
        if let Some((identity, action)) = action {
            if matches!(action, LocalFormAction::SubmitButton(_)) {
                crate::audio::ui_click();
            }
            let _ = runtime.respond_to_server_form(identity, action);
        }
    } else {
        wheel.clear();
        keyboard.clear();
    }
    // Also retain ownership through the answer frame; pending enqueue owns
    // input until the later network phase accepts it or retires the session.
    client_ui::ui_runtime::interaction::suppress_gameplay_input_for_chat(
        &player_runtime,
        &runtime,
        &mut cursor,
        &mut keys,
        &mut mouse,
        &mut motion,
    );
}
