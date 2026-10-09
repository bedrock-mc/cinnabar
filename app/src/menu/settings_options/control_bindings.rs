//! Bevy device adapters for the launcher's persisted physical bindings.
use launcher::menu::settings_options::{EXTRA_GAMEPAD, GAMEPAD_BINDINGS, GAMEPAD_OFFSET};
use semantic_input::PhysicalControl;

/// Inventory shortcuts share the exact configured controls used by gameplay slots.
pub(crate) fn hotbar_control_slot(
    menu: Option<&crate::menu::MenuRuntime>,
    control: PhysicalControl,
) -> Option<u8> {
    crate::hotbar::HOTBAR_DIGIT_ACTIONS
        .iter()
        .enumerate()
        .find_map(|(slot, action)| {
            let (_, name) = super::KEY_BINDINGS
                .iter()
                .find(|(candidate, _)| candidate == action)?;
            (named_control(menu, name) == Some(control)).then_some(slot as u8)
        })
}

/// Resolves a gameplay UI control from the menu's persisted settings or startup defaults.
pub(crate) fn named_control(
    menu: Option<&crate::menu::MenuRuntime>,
    name: &str,
) -> Option<PhysicalControl> {
    menu.map_or_else(
        || super::SettingsOptions::default().named_key_control(name),
        |menu| menu.settings_options.named_key_control(name),
    )
}

/// Matches a keyboard event without changing the physical key used for text entry.
pub(crate) fn binding_key(
    menu: Option<&crate::menu::MenuRuntime>,
    name: &str,
    key: bevy::prelude::KeyCode,
) -> bool {
    crate::semantic_controls::keyboard_usage(key).is_some_and(|code| {
        let control = PhysicalControl::KeyboardUsage(code);
        named_control(menu, name) == Some(control)
            || menu.map_or_else(
                || super::SettingsOptions::default().secondary_key_control(name),
                |menu| menu.settings_options.secondary_key_control(name),
            ) == Some(control)
    })
}

/// Tests one configured keyboard or mouse action in the production device frame.
pub(crate) fn binding_pressed(
    menu: Option<&crate::menu::MenuRuntime>,
    name: &str,
    keys: &bevy::prelude::ButtonInput<bevy::prelude::KeyCode>,
    mouse: &bevy::prelude::ButtonInput<bevy::prelude::MouseButton>,
) -> bool {
    if keys
        .get_just_pressed()
        .any(|key| binding_key(menu, name, *key))
    {
        return true;
    }
    match named_control(menu, name) {
        Some(PhysicalControl::MouseButton(code)) => mouse.get_just_pressed().any(|button| {
            crate::semantic_controls::physical::mouse_button_code(*button) == Some(code)
        }),
        _ => false,
    }
}

/// Matches mouse-only UI actions before keyboard events are processed.
pub(crate) fn binding_mouse(
    menu: Option<&crate::menu::MenuRuntime>,
    name: &str,
    mouse: &bevy::prelude::ButtonInput<bevy::prelude::MouseButton>,
) -> bool {
    mouse
        .get_just_pressed()
        .any(|button| binding_mouse_button(menu, name, *button))
}

/// Whether the control named `name` is bound to `button`.
pub(crate) fn binding_mouse_button(
    menu: Option<&crate::menu::MenuRuntime>,
    name: &str,
    button: bevy::prelude::MouseButton,
) -> bool {
    matches!(named_control(menu, name), Some(PhysicalControl::MouseButton(code))
        if crate::semantic_controls::physical::mouse_button_code(button) == Some(code))
}

/// Reads a gamepad UI action from the same persisted layout as the settings grid.
pub(crate) fn binding_gamepad(
    menu: Option<&crate::menu::MenuRuntime>,
    name: &str,
    pads: &bevy::prelude::Query<&bevy::input::gamepad::Gamepad>,
) -> bool {
    let Some(index) = EXTRA_GAMEPAD.iter().position(|(label, _)| *label == name) else {
        return false;
    };
    let row = GAMEPAD_OFFSET + GAMEPAD_BINDINGS.len() + index;
    let control = menu.map_or_else(
        || super::SettingsOptions::default().key_control(row),
        |menu| menu.settings_options.key_control(row),
    );
    pads.iter().any(|pad| match control {
        Some(PhysicalControl::GamepadButton(code)) => {
            crate::semantic_controls::physical::TRANSLATED_GAMEPAD_BUTTONS
                .iter()
                .any(|(button_code, button)| *button_code == code && pad.just_pressed(*button))
        }
        Some(PhysicalControl::GamepadAxis { axis, .. }) => pad.just_pressed(if axis == 4 {
            bevy::input::gamepad::GamepadButton::LeftTrigger2
        } else {
            bevy::input::gamepad::GamepadButton::RightTrigger2
        }),
        _ => false,
    })
}

/// Uses the same swap mapping for menu confirmation as for gameplay.
pub(crate) fn gamepad_button(
    settings: &super::SettingsOptions,
    button: bevy::input::gamepad::GamepadButton,
) -> bevy::input::gamepad::GamepadButton {
    let table = crate::semantic_controls::physical::TRANSLATED_GAMEPAD_BUTTONS;
    let Some((code, _)) = table.iter().find(|(_, candidate)| *candidate == button) else {
        return button;
    };
    let swapped = settings.swap_gamepad_control(PhysicalControl::GamepadButton(*code));
    table
        .iter()
        .find(|(code, _)| swapped == PhysicalControl::GamepadButton(*code))
        .map_or(button, |(_, button)| *button)
}
