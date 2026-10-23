//! Session, clock and desktop authority used by menu input.

#[cfg(any(windows, target_os = "macos", target_os = "linux", test))]
use bevy::input::keyboard::Key;
use bevy::{
    ecs::message::{MessageCursor, Messages},
    input::{
        keyboard::{KeyboardFocusLost, KeyboardInput},
        touch::TouchInput,
    },
    prelude::{ButtonInput, KeyCode, Local, MessageReader, Real, Res, ResMut, Time},
    winit::RawWinitWindowEvent,
};
use std::collections::HashSet;

/// Groups the session and desktop authority read before menu actions.
#[derive(bevy::ecs::system::SystemParam)]
pub struct MenuInputContext<'w, 's> {
    pub(super) player_runtime: Res<'w, client_state::PlayerRuntime>,
    pub(super) time: Option<Res<'w, Time<Real>>>,
    pub(super) keyboard_messages: MessageReader<'w, 's, KeyboardInput>,
    pub(super) touch_messages: Option<Res<'w, Messages<TouchInput>>>,
    pub(super) touch_cursor: Local<'s, MessageCursor<TouchInput>>,
    pub(super) focus: Option<ResMut<'w, client_state::CursorFocus>>,
    pub(super) driven: Option<Res<'w, client_state::DrivenInput>>,
}

/// Tracks modifiers and native input without consuming menu navigation's messages.
#[derive(Default)]
pub(crate) struct KeyLabelInput {
    modifiers: super::MenuModifiers,
    native_cursor: MessageCursor<RawWinitWindowEvent>,
    focus_cursor: MessageCursor<KeyboardFocusLost>,
}

/// Learns native unmodified characters, or ignores modified fallback characters.
pub(crate) fn observe_keyboard_labels(
    mut messages: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    native: Option<Res<Messages<RawWinitWindowEvent>>>,
    focus_lost: Option<Res<Messages<KeyboardFocusLost>>>,
    mut input: Local<KeyLabelInput>,
    mut menu: ResMut<crate::menu::MenuRuntime>,
) {
    let mut normalized = HashSet::new();
    if let Some(native) = native {
        for event in input.native_cursor.read(&native) {
            if let Some(key) = observe_native_event(&mut menu, event) {
                normalized.insert(key);
            }
        }
    }
    input.modifiers.capture_pressed(&keys);
    for event in messages.read() {
        input.modifiers.observe(event);
        if input.modifiers.0 == 0 && !normalized.contains(&event.key_code) {
            super::remapping::observe_label(&mut menu, event);
        }
    }
    if let Some(focus_lost) = focus_lost
        && input.focus_cursor.read(&focus_lost).count() != 0
    {
        input.modifiers = super::MenuModifiers::default();
    }
}

/// Converts native desktop presses using the layout character before modifier effects.
fn observe_native_event(
    menu: &mut crate::menu::MenuRuntime,
    event: &RawWinitWindowEvent,
) -> Option<KeyCode> {
    #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
    {
        use bevy::{
            prelude::Entity,
            winit::converters::{convert_keyboard_input, convert_logical_key},
        };
        use winit::{
            event::WindowEvent, platform::modifier_supplement::KeyEventExtModifierSupplement,
        };

        if let WindowEvent::KeyboardInput { event, .. } = &event.event {
            return learn_native_label(
                menu,
                convert_keyboard_input(event, Entity::PLACEHOLDER),
                convert_logical_key(&event.key_without_modifiers()),
            );
        }
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    let _ = (menu, event);
    None
}

/// Prefers a usable native character over the matching modifier-affected Bevy event.
#[cfg(any(windows, target_os = "macos", target_os = "linux", test))]
fn learn_native_label(
    menu: &mut crate::menu::MenuRuntime,
    mut input: KeyboardInput,
    unmodified: Key,
) -> Option<KeyCode> {
    if input.state != bevy::input::ButtonState::Pressed
        || !matches!(unmodified, Key::Character(_) | Key::Dead(Some(_)))
    {
        return None;
    }
    input.logical_key = unmodified;
    super::remapping::observe_label(menu, &input);
    Some(input.key_code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{input::ButtonState, prelude::Entity};
    use semantic_input::PhysicalControl;

    #[test]
    fn native_label_uses_the_character_without_modifiers() {
        let mut menu = crate::menu::MenuRuntime::new(false, 2, "Bindings".into());
        for (physical, modified, unmodified, expected) in [
            (KeyCode::Digit1, "!", "1", "1"),
            (KeyCode::Digit2, "@", "2", "2"),
            (KeyCode::KeyQ, "A", "a", "A"),
        ] {
            let key = learn_native_label(
                &mut menu,
                KeyboardInput {
                    key_code: physical,
                    logical_key: Key::Character(modified.into()),
                    state: ButtonState::Pressed,
                    text: Some(modified.into()),
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                },
                Key::Character(unmodified.into()),
            );
            assert_eq!(key, Some(physical));
            let usage = crate::physical_input::keyboard_usage(physical).unwrap();
            assert_eq!(
                menu.settings_options
                    .key_name(PhysicalControl::KeyboardUsage(usage)),
                expected
            );
        }
    }
}
