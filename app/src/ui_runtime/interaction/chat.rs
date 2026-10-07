//! Mouse and gamepad input for the native chat screen and its settings popup.

use super::*;
use client_ui::ui_runtime::presentation::{BedHit, ChatHit, UiPresentationRuntime};

/// Routes chat pointer actions while a settings modal owns its underlying controls.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_chat_ui_actions(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    time: Res<Time<Real>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut menu: Option<ResMut<crate::menu::MenuRuntime>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    wheel: Option<Res<AccumulatedMouseScroll>>,
    touches: Res<Touches>,
    gamepads: Query<&Gamepad>,
    coordinates: super::chat_coordinates::ChatCoordinateContext,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut runtime: ResMut<UiRuntime>,
    mut focus: Option<ResMut<client_presentation::camera::CursorFocus>>,
    driven: Option<Res<crate::camera::DrivenInput>>,
) {
    if runtime.credits().owns_input() || runtime.server_forms().owns_input() {
        return;
    }
    let input_available = driven.is_some()
        || (window.focused && focus.as_ref().is_none_or(|focus| focus.available()));
    let pointer = window
        .cursor_position()
        .and_then(|position| UiPoint::new(position.x, position.y).ok());
    let menu_visible = menu.as_ref().is_some_and(|menu| menu.is_visible());
    let bed =
        !menu_visible && input_available && runtime.local_sleeping() && !runtime.chat_focused();
    presentation.set_bed_pointer(pointer.filter(|_| bed));
    if bed && mouse_buttons.just_pressed(MouseButton::Left) {
        match pointer.and_then(|position| presentation.hit_test_bed(position)) {
            Some(BedHit::LeaveBed) => {
                runtime.request_wake();
                if let Some(focus) = focus.as_deref_mut() {
                    focus.authorize_screen_return();
                }
            }
            Some(BedHit::OpenChat) => {
                runtime.open_chat(&mut player_runtime);
            }
            None => {}
        }
        return;
    }
    if menu_visible || !runtime.chat_focused() || !input_available {
        presentation.set_chat_pointer(None);
        return;
    }
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    coordinates.publish(&player_runtime, &runtime, &mut presentation);
    presentation.set_chat_pointer(pointer);
    if let Some(wheel) = wheel.as_deref()
        && wheel.delta.y != 0.0
    {
        presentation.scroll_chat(wheel.delta.y, wheel.unit == MouseScrollUnit::Pixel);
    }

    let mut presses: Vec<UiPoint> = Vec::new();
    if mouse_buttons.just_pressed(MouseButton::Left)
        && let Some(position) = pointer
    {
        presses.push(position);
    }
    for touch in touches.iter_just_pressed() {
        let position = touch.position();
        if let Ok(position) = UiPoint::new(position.x, position.y) {
            presses.push(position);
        }
    }
    for position in presses {
        let hit = presentation.hit_test_chat(position);
        match hit {
            Some(ChatHit::Link(index)) => presentation.request_chat_link(index),
            Some(ChatHit::LinkOpen) => {
                if let Some(url) = presentation.take_confirmed_chat_link() {
                    crate::desktop::open_url(&url);
                }
            }
            Some(ChatHit::LinkCancel) => presentation.cancel_chat_link(),
            _ if presentation.chat_link_confirmation_open() => {}
            Some(ChatHit::SettingsOpen) => presentation.set_chat_settings_open(true),
            Some(ChatHit::SettingsClose) => presentation.set_chat_settings_open(false),
            Some(ChatHit::SettingsAction(action)) => {
                if let Some(menu) = menu.as_deref_mut() {
                    menu.activate(action);
                }
            }
            _ if presentation.chat_settings_open() => {}
            Some(ChatHit::CoordinateDropdown) => {
                presentation.select_chat_coordinates(None);
            }
            Some(ChatHit::CoordinateSource(facing)) => {
                presentation.select_chat_coordinates(Some(facing));
            }
            Some(ChatHit::CopyCoordinates) => {
                if let Some(text) = presentation.chat_coordinate_text()
                    && PlatformClipboard.write_text(text).is_ok()
                {
                    presentation.chat_coordinates_copied(now_millis);
                }
            }
            Some(ChatHit::Paste) => {
                let _ = runtime.paste_chat_text(&mut PlatformClipboard);
            }
            Some(ChatHit::Send) => {
                dispatch_chat_ui_action(&mut runtime, UiAction::Accept, None, now_millis);
            }
            Some(ChatHit::Close) => {
                dispatch_chat_ui_action(&mut runtime, UiAction::Cancel, None, now_millis);
            }
            _ => {
                let suggestion = match hit {
                    Some(ChatHit::Suggestion(index)) => Some(index),
                    _ => None,
                };
                dispatch_chat_ui_action(
                    &mut runtime,
                    UiAction::PointerPrimary {
                        position,
                        phase: PointerPhase::Pressed,
                    },
                    suggestion,
                    now_millis,
                );
            }
        }
    }
    for gamepad in &gamepads {
        for button in [
            GamepadButton::DPadUp,
            GamepadButton::DPadDown,
            GamepadButton::South,
            GamepadButton::East,
            GamepadButton::RightTrigger,
            GamepadButton::LeftTrigger,
        ] {
            if gamepad.just_pressed(button) {
                if presentation.chat_link_confirmation_open() {
                    if button == GamepadButton::East {
                        presentation.cancel_chat_link();
                    }
                    continue;
                }
                if presentation.chat_settings_open() {
                    if button == GamepadButton::East {
                        presentation.set_chat_settings_open(false);
                    }
                    continue;
                }
                dispatch_chat_ui_action(
                    &mut runtime,
                    gamepad_chat_action(button).expect("the mapped button list is exhaustive"),
                    None,
                    now_millis,
                );
            }
        }
    }
    if !runtime.chat_focused()
        && let Some(focus) = focus.as_deref_mut()
    {
        focus.authorize_screen_return();
    }
}
