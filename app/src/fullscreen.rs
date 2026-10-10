//! Desktop fullscreen state shared by the video settings and F11.
//! The pinned Bedrock `ui/settings_sections/general_section.json` control is
//! `full_screen`; entering fullscreen uses the current monitor without changing
//! its display mode.

use bevy::{
    input::{ButtonState, keyboard::KeyboardInput},
    prelude::{Entity, Local, MessageReader, Query, ResMut, With},
    window::{MonitorSelection, PrimaryWindow, Window, WindowMode},
};

use crate::{menu::MenuRuntime, settings_runtime::RuntimeSettings};

/// Read before menu input clears its keyboard state. OS key repeats must not
/// flip fullscreen again while F11 is held, including on a visible menu.
pub(crate) fn toggle_fullscreen_hotkey(
    mut keyboard: MessageReader<KeyboardInput>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut settings: ResMut<RuntimeSettings>,
    mut menu: ResMut<MenuRuntime>,
) {
    let Ok((entity, mut window)) = windows.single_mut() else {
        keyboard.clear();
        return;
    };
    if !window.visible {
        keyboard.clear();
        return;
    }
    let toggles = keyboard
        .read()
        .filter(|input| {
            input.window == entity
                && crate::menu::settings_options::control_bindings::binding_key(
                    Some(&menu),
                    "key.fullscreen",
                    input.key_code,
                )
                && input.state == ButtonState::Pressed
                && !input.repeat
        })
        .count();
    if !window.focused || toggles % 2 == 0 {
        return;
    }
    let fullscreen = !menu
        .take_fullscreen_change()
        .unwrap_or_else(|| is_fullscreen(&window));
    set_fullscreen(&mut window, fullscreen);
    record_fullscreen(&mut settings, fullscreen);
    menu.sync_fullscreen(fullscreen);
}

/// Apply menu presses and retained settings before the UI publishes its next
/// frame, so the checkbox and the native primary window always agree.
pub(crate) fn apply_runtime_fullscreen_setting(
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut settings: ResMut<RuntimeSettings>,
    mut menu: ResMut<MenuRuntime>,
    mut observed_generation: Local<u64>,
) {
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    // A hidden capture window keeps its requested size and leaves the saved preference alone.
    if !window.visible {
        return;
    }
    if let Some(fullscreen) = menu.take_fullscreen_change() {
        set_fullscreen(&mut window, fullscreen);
        record_fullscreen(&mut settings, fullscreen);
    } else {
        let (generation, user_settings) = settings.user_settings_update();
        if generation > *observed_generation {
            set_fullscreen(&mut window, user_settings.video.fullscreen);
        } else {
            record_fullscreen(&mut settings, is_fullscreen(&window));
        }
    }
    *observed_generation = settings.user_settings_update().0;
    menu.sync_fullscreen(is_fullscreen(&window));
}

fn is_fullscreen(window: &Window) -> bool {
    window.mode != WindowMode::Windowed
}

fn set_fullscreen(window: &mut Window, fullscreen: bool) {
    if fullscreen != is_fullscreen(window) {
        window.mode = if fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        };
    }
}

fn record_fullscreen(settings: &mut RuntimeSettings, fullscreen: bool) {
    settings.set_fullscreen(fullscreen);
}

#[cfg(test)]
mod tests;
