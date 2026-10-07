mod remapping;

use bevy::{
    ecs::message::{MessageCursor, Messages},
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
        mouse::{MouseButtonInput, MouseScrollUnit, MouseWheel},
        touch::Touches,
    },
    prelude::{
        ButtonInput, Entity, KeyCode, Local, MessageReader, MouseButton, Query, Res, ResMut,
        Resource, Single, With,
    },
    window::{CursorOptions, PrimaryWindow, Window},
};
use ui::{ChatClipboard, ChatEditor, UiPoint};

use super::{
    MAX_SERVER_ADDRESS_BYTES, MAX_SERVER_NAME_BYTES, MAX_SERVER_PORT_BYTES, MenuField, MenuRuntime,
};
use crate::local_worlds::{MAX_SEED_CHARS, MAX_WORLD_NAME_CHARS};
use client_ui::ui_runtime::{PlatformClipboard, presentation::UiPresentationRuntime};
use launcher::menu::view::MenuCaret;

/// Vanilla's fixed desktop option hotkeys.
pub(crate) const HOTKEY_OPTIONS: [(KeyCode, &str); 2] =
    [(KeyCode::F1, "hide_hud"), (KeyCode::F8, "hide_paperdoll")];

#[derive(Resource)]
pub(crate) struct MenuClipboard(
    Box<dyn FnMut(usize) -> Option<String> + Send + Sync + 'static>,
    Box<dyn FnMut(String) + Send + Sync + 'static>,
);

impl MenuClipboard {
    pub(crate) fn with_access(
        reader: impl FnMut(usize) -> Option<String> + Send + Sync + 'static,
        writer: impl FnMut(String) + Send + Sync + 'static,
    ) -> Self {
        Self(Box::new(reader), Box::new(writer))
    }

    fn read_text_bounded(&mut self, maximum_bytes: usize) -> Option<String> {
        (self.0)(maximum_bytes)
    }

    pub(crate) fn write_text(&mut self, text: String) {
        (self.1)(text);
    }
}

impl Default for MenuClipboard {
    fn default() -> Self {
        let mut reader = PlatformClipboard;
        let mut writer = PlatformClipboard;
        Self::with_access(
            move |maximum_bytes| {
                reader
                    .read_text_bounded(maximum_bytes)
                    .ok()
                    .flatten()
                    .map(|text| text.to_string())
            },
            move |text| {
                let _ = writer.write_text(text);
            },
        )
    }
}

impl ChatClipboard for MenuClipboard {
    type Error = std::convert::Infallible;

    fn read_text_bounded(
        &mut self,
        maximum_bytes: usize,
    ) -> Result<Option<std::sync::Arc<str>>, Self::Error> {
        Ok(self
            .read_text_bounded(maximum_bytes)
            .filter(|text| text.len() <= maximum_bytes)
            .map(std::sync::Arc::from))
    }
}

#[derive(Default)]
pub(crate) struct MenuModifiers(u8);

/// Retains physical button state because menu input clears Bevy's buttons
/// after consumption, and retains slider capture across UI scale relayout.
#[derive(Default)]
pub(crate) struct GuiScaleDrag {
    mouse_cursor: MessageCursor<MouseButtonInput>,
    left_held: bool,
    captured: bool,
}

impl MenuModifiers {
    const CONTROL_LEFT: u8 = 1 << 0;
    const CONTROL_RIGHT: u8 = 1 << 1;
    const SUPER_LEFT: u8 = 1 << 2;
    const SUPER_RIGHT: u8 = 1 << 3;
    const ALT_LEFT: u8 = 1 << 4;
    const ALT_RIGHT: u8 = 1 << 5;
    const SHIFT_LEFT: u8 = 1 << 6;
    const SHIFT_RIGHT: u8 = 1 << 7;

    fn capture_pressed(&mut self, keys: &ButtonInput<KeyCode>) {
        for key in [
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
        ] {
            if keys.pressed(key) {
                self.0 |= Self::mask(key);
            }
        }
    }

    fn observe(&mut self, input: &KeyboardInput) {
        let mask = Self::mask(input.key_code);
        if input.state == ButtonState::Pressed {
            self.0 |= mask;
        } else {
            self.0 &= !mask;
        }
    }

    fn shortcut(&self) -> bool {
        self.0 & 0b0000_1111 != 0 && self.0 & 0b0011_0000 == 0
    }

    fn shift(&self) -> bool {
        self.0 & 0b1100_0000 != 0
    }

    const fn mask(key: KeyCode) -> u8 {
        match key {
            KeyCode::ControlLeft => Self::CONTROL_LEFT,
            KeyCode::ControlRight => Self::CONTROL_RIGHT,
            KeyCode::SuperLeft => Self::SUPER_LEFT,
            KeyCode::SuperRight => Self::SUPER_RIGHT,
            KeyCode::AltLeft => Self::ALT_LEFT,
            KeyCode::AltRight => Self::ALT_RIGHT,
            KeyCode::ShiftLeft => Self::SHIFT_LEFT,
            KeyCode::ShiftRight => Self::SHIFT_RIGHT,
            _ => 0,
        }
    }
}

/// The editor `field` types into, bounded by its byte budget. The menu boxes share the
/// chat box's caret model: a byte caret on character boundaries plus a selection.
pub(super) fn field_editor(field: MenuField) -> ChatEditor {
    ChatEditor::new(max_bytes(field)).expect("menu field budgets are valid editor bounds")
}

impl MenuRuntime {
    /// Focus `field` with its caret after the text, as a newly selected box.
    pub(super) fn focus_field(&mut self, field: MenuField) {
        self.field = Some(field);
        self.edit_field(|editor| editor.move_end(false));
    }

    fn has_focused_field(&self) -> bool {
        self.field.is_some()
    }

    fn editor(&self, field: MenuField) -> &ChatEditor {
        match field {
            MenuField::Name => &self.name,
            MenuField::Address => &self.address,
            MenuField::Port => &self.port,
            MenuField::WorldName => &self.local_ui.name,
            MenuField::WorldSeed => &self.local_ui.seed,
        }
    }

    fn editor_mut(&mut self, field: MenuField) -> &mut ChatEditor {
        match field {
            MenuField::Name => &mut self.name,
            MenuField::Address => &mut self.address,
            MenuField::Port => &mut self.port,
            MenuField::WorldName => &mut self.local_ui.name,
            MenuField::WorldSeed => &mut self.local_ui.seed,
        }
    }

    /// `field`'s current text.
    pub(crate) fn field_text(&self, field: MenuField) -> &str {
        self.editor(field).as_str()
    }

    pub(super) fn caret(&self) -> MenuCaret {
        MenuCaret {
            byte: self
                .field
                .map_or(0, |field| self.editor(field).cursor_byte()),
            selection: self
                .field
                .and_then(|field| self.editor(field).selection())
                .map(|range| [range.start, range.end]),
            revision: self.caret_revision,
            shown: true,
        }
    }

    /// Apply `edit` to the focused field; every edit or caret move restarts the blink,
    /// as vanilla's text edit box shows its caret again after typing.
    fn edit_field(&mut self, edit: impl FnOnce(&mut ChatEditor)) {
        let Some(field) = self.field else {
            return;
        };
        edit(self.editor_mut(field));
        self.caret_revision = self.caret_revision.wrapping_add(1);
    }

    /// A press inside the focused field puts its caret at `byte`.
    pub(crate) fn place_caret(&mut self, byte: usize) {
        self.edit_field(|editor| editor.place_cursor(byte));
    }

    fn select_all_text(&mut self) {
        self.edit_field(|editor| {
            editor.move_home(false);
            editor.move_end(true);
        });
    }

    fn selected_text(&self) -> Option<&str> {
        let editor = self.editor(self.field?);
        editor.selection().map(|range| &editor.as_str()[range])
    }

    fn remaining_text_capacity(&self) -> usize {
        self.field
            .map_or(0, |field| self.editor(field).remaining_insert_capacity())
    }

    /// Insert what fits of `text` at the caret, replacing any selection.
    fn edit_text(&mut self, text: &str) {
        let Some(field) = self.field else {
            return;
        };
        // The port box takes number characters only, as vanilla's `NumberChars` text type.
        let accepts = |character: &char| field != MenuField::Port || character.is_ascii_digit();
        self.edit_field(|editor| {
            let capacity = editor.remaining_insert_capacity();
            let mut insertion = String::new();
            for character in text
                .chars()
                .filter(|character| !character.is_control())
                .filter(accepts)
            {
                if insertion.len() + character.len_utf8() > capacity {
                    break;
                }
                insertion.push(character);
            }
            // Within the remaining capacity, so the insert cannot be refused; nothing
            // accepted leaves a selection standing.
            if !insertion.is_empty() {
                let _ = editor.insert(&insertion);
            }
        });
    }

    /// A caret or deletion key in the focused field; `false` leaves it to focus navigation.
    fn edit_key(&mut self, key: KeyCode, selecting: bool) -> bool {
        let edit: fn(&mut ChatEditor, bool) = match key {
            KeyCode::ArrowLeft => |editor, selecting| {
                if selecting {
                    editor.select_left();
                } else {
                    editor.move_left();
                }
            },
            KeyCode::ArrowRight => |editor, selecting| {
                if selecting {
                    editor.select_right();
                } else {
                    editor.move_right();
                }
            },
            KeyCode::Home => ChatEditor::move_home,
            KeyCode::End => ChatEditor::move_end,
            KeyCode::Backspace => |editor, _| editor.backspace(),
            KeyCode::Delete => |editor, _| editor.delete(),
            _ => return false,
        };
        if !self.has_focused_field() {
            return false;
        }
        self.edit_field(|editor| edit(editor, selecting));
        true
    }
}

/// A field's byte budget; world fields allow their vanilla character limits in any script.
fn max_bytes(field: MenuField) -> usize {
    match field {
        MenuField::Name => MAX_SERVER_NAME_BYTES,
        MenuField::Address => MAX_SERVER_ADDRESS_BYTES,
        MenuField::Port => MAX_SERVER_PORT_BYTES,
        MenuField::WorldName => MAX_WORLD_NAME_CHARS * 4,
        MenuField::WorldSeed => MAX_SEED_CHARS,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_menu_input(
    (player_runtime, mut keyboard_messages): (
        bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
        MessageReader<KeyboardInput>,
    ),
    wheel_messages: Option<Res<Messages<MouseWheel>>>,
    mut wheel_cursor: Local<MessageCursor<MouseWheel>>,
    window: Single<(Entity, &Window, &mut CursorOptions), With<PrimaryWindow>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    touches: Res<Touches>,
    gamepads: Query<&Gamepad>,
    mut presentation: ResMut<UiPresentationRuntime>,
    mut clipboard: ResMut<MenuClipboard>,
    mut menu: ResMut<MenuRuntime>,
    runtime: Option<Res<client_ui::ui_runtime::UiRuntime>>,
    mut modifiers: Local<MenuModifiers>,
    consent: Option<Res<crate::server_experiences::input::ConsentInput>>,
    mouse_messages: Option<Res<Messages<MouseButtonInput>>>,
    mut gui_scale_drag: Local<GuiScaleDrag>,
) {
    if consent.is_some_and(|consent| consent.0) {
        keyboard_messages.clear();
        menu.pressed = None;
        menu.hovered = None;
        menu.pointer_down = false;
        *modifiers = MenuModifiers::default();
        gui_scale_drag.captured = false;
        gui_scale_drag.left_held = false;
        if let Some(messages) = mouse_messages.as_deref() {
            gui_scale_drag.mouse_cursor.clear(messages);
        }
        if let Some(messages) = wheel_messages.as_deref() {
            wheel_cursor.clear(messages);
        }
        return;
    }
    menu.refresh_settings_focus(presentation.menu_focus_actions());
    let (window_entity, window, mut cursor) = window.into_inner();
    if let Some(messages) = mouse_messages.as_deref() {
        let GuiScaleDrag {
            mouse_cursor,
            left_held,
            ..
        } = &mut *gui_scale_drag;
        for input in mouse_cursor.read(messages) {
            if input.window == window_entity && input.button == MouseButton::Left {
                *left_held = input.state == ButtonState::Pressed;
            }
        }
    }
    let wheel: Vec<(f32, bool)> = wheel_messages
        .as_deref()
        .map(|messages| {
            wheel_cursor
                .read(messages)
                .map(|wheel| (wheel.y, wheel.unit == MouseScrollUnit::Pixel))
                .collect()
        })
        .unwrap_or_default();
    if runtime.as_ref().is_some_and(|runtime| {
        runtime.credits().owns_input()
            || runtime.server_forms().owns_input()
                && (!menu.is_visible() || runtime.server_forms().settings_form_active())
    }) {
        gui_scale_drag.captured = false;
        gui_scale_drag.left_held = false;
        keyboard_messages.clear();
        return;
    }
    menu.pressed = None;
    if !window.focused {
        gui_scale_drag.captured = false;
        gui_scale_drag.left_held = false;
        if !menu.is_visible()
            && menu.settings_options.value("pause_menu_on_focus_lost") != 0
            && !crate::screen_policy::absorbs_input(
                &player_runtime,
                runtime.as_deref(),
                Some(&menu),
                Some(&presentation),
            )
        {
            menu.open_pause();
            crate::camera::release_cursor(&mut cursor);
        }
        *modifiers = MenuModifiers::default();
        keyboard_messages.clear();
        menu.pointer_down = false;
        return;
    }
    // Zero health in play opens the death screen; recovery closes it.
    if let Some(health) = runtime.as_ref().and_then(|runtime| runtime.hud().health()) {
        if health.current() == 0 {
            menu.open_death();
        } else {
            menu.note_player_alive();
        }
    }
    if !menu.is_visible()
        && runtime
            .as_ref()
            .is_none_or(|runtime| !runtime.ui_focused(&player_runtime))
    {
        // Vanilla desktop input uses fixed F1/F8 shortcuts.
        for (key, option) in HOTKEY_OPTIONS {
            if keys.just_pressed(key) {
                let value = 1 - menu.settings_options.value(option);
                if menu.transient_toggles {
                    menu.set_session_option(option, Some(value));
                } else {
                    menu.set_named_option(option, value);
                }
            }
        }
    }
    if !menu.is_visible() {
        gui_scale_drag.captured = false;
        // Gameplay/chat handled these messages already. In particular, do not
        // replay the Escape that opens pause as "back" on the following frame.
        *modifiers = MenuModifiers::default();
        keyboard_messages.clear();
        menu.hovered = None;
        menu.pointer_down = false;
        if keys.just_pressed(KeyCode::Escape) {
            modifiers.capture_pressed(&keys);
            menu.open_pause();
            crate::camera::release_cursor(&mut cursor);
            keys.reset_all();
        }
        return;
    }

    if menu.key_remap.is_some() {
        remapping::capture(
            &mut menu,
            &mut keyboard_messages,
            &mut keys,
            &mut mouse_buttons,
            &gamepads,
        );
        return;
    }

    modifiers.capture_pressed(&keys);
    crate::camera::release_cursor(&mut cursor);
    let pointer = window
        .cursor_position()
        .and_then(|position| UiPoint::new(position.x, position.y).ok());
    menu.hovered = pointer.and_then(|position| presentation.hit_test_menu(position));
    let pointer_pressed = mouse_buttons.pressed(MouseButton::Left) || gui_scale_drag.left_held;
    let pointer_just_pressed =
        mouse_buttons.just_pressed(MouseButton::Left) || (pointer_pressed && !menu.pointer_down);
    menu.pointer_down = pointer_pressed;
    if !pointer_pressed || menu.screen() != super::MenuScreen::Settings {
        gui_scale_drag.captured = false;
    }
    if let Some(point) = pointer {
        for (notches, pixels) in wheel {
            presentation.scroll_menu(point, notches, pixels);
        }
    }
    // A scrollbar press or drag scrolls instead of pressing what lies beneath.
    let on_scrollbar = presentation.drag_menu_scroll(pointer, pointer_pressed)
        || (pointer_just_pressed
            && pointer.is_some_and(|point| presentation.press_menu_scrollbar(point)));
    if on_scrollbar {
        menu.hovered = None;
    }
    let press = |menu: &mut MenuRuntime, action| {
        if let Some(sound) = presentation.menu_sound(action) {
            crate::audio::ui_control_sound(sound);
        }
        menu.activate(action);
    };
    if !pointer_pressed || menu.screen() != super::MenuScreen::Settings {
        menu.settings_slider_drag = None;
    }
    if pointer_just_pressed {
        menu.settings_slider_drag = match menu.hovered {
            Some(super::MenuAction::SettingsOption(index, _))
                if matches!(
                    super::settings_options::SETTINGS_OPTIONS[usize::from(index)].kind,
                    super::settings_options::SettingKind::Slider
                ) =>
            {
                Some(index)
            }
            _ => None,
        };
    }
    if pointer_pressed
        && !pointer_just_pressed
        && let Some(index) = menu.settings_slider_drag
        && let Some(super::MenuAction::SettingsOption(_, value)) =
            pointer.and_then(|point| presentation.settings_slider_drag_action(index, point))
    {
        menu.set_option(index, value);
    }
    if pointer_just_pressed
        && !on_scrollbar
        && matches!(menu.hovered, Some(super::MenuAction::SettingsScale(_)))
    {
        gui_scale_drag.captured = pointer_pressed;
    }
    if gui_scale_drag.captured
        && pointer_pressed
        && let Some(action @ super::MenuAction::SettingsScale(offset)) =
            pointer.and_then(|point| presentation.gui_scale_drag_action(point))
    {
        menu.hovered = Some(action);
        if pointer_just_pressed || offset != menu.gui_scale_offset() {
            menu.activate(action);
        } else {
            menu.pressed = Some(action);
        }
    }
    // A press inside a text box also puts its caret at the nearest character.
    let mut caret_press = None;
    if pointer_just_pressed
        && !on_scrollbar
        && !gui_scale_drag.captured
        && let Some(action) = menu.hovered
    {
        press(&mut menu, action);
        caret_press = pointer.zip(action.text_field());
    }
    for touch in touches.iter_just_pressed() {
        let position = touch.position();
        if let Ok(position) = UiPoint::new(position.x, position.y)
            && let Some(action) = presentation.hit_test_menu(position)
        {
            press(&mut menu, action);
            caret_press = action.text_field().map(|field| (position, field));
        }
    }
    if let Some((point, field)) = caret_press
        && menu.field == Some(field)
        && let Some(byte) = presentation.menu_caret_at(point, field, menu.field_text(field))
    {
        menu.place_caret(byte);
    }
    for gamepad in &gamepads {
        if gamepad.just_pressed(GamepadButton::DPadLeft) {
            menu.move_horizontal_focus(-1);
        }
        if gamepad.just_pressed(GamepadButton::DPadRight) {
            menu.move_horizontal_focus(1);
        }
        if gamepad.just_pressed(GamepadButton::DPadUp) {
            menu.move_focus(-1);
        }
        if gamepad.just_pressed(GamepadButton::DPadDown) {
            menu.move_focus(1);
        }
        if gamepad.just_pressed(super::settings_options::gamepad_button(
            &menu.settings_options,
            GamepadButton::South,
        )) {
            menu.activate_focused();
        }
        if gamepad.just_pressed(super::settings_options::gamepad_button(
            &menu.settings_options,
            GamepadButton::East,
        )) {
            menu.go_back_from_input();
        }
    }
    for input in keyboard_messages.read() {
        modifiers.observe(input);
        if input.state != ButtonState::Pressed {
            continue;
        }
        if modifiers.shortcut() && menu.has_focused_field() {
            match input.key_code {
                KeyCode::KeyA => {
                    menu.select_all_text();
                    continue;
                }
                KeyCode::KeyC => {
                    if let Some(text) = menu.selected_text() {
                        clipboard.write_text(text.to_owned());
                    }
                    continue;
                }
                KeyCode::KeyV => {
                    let maximum = menu.remaining_text_capacity();
                    if let Some(text) = clipboard.read_text_bounded(maximum) {
                        menu.edit_text(&text);
                    }
                    continue;
                }
                _ => {}
            }
            if input.text.is_some() {
                continue;
            }
        }
        if menu.edit_key(input.key_code, modifiers.shift()) {
            continue;
        }
        match input.key_code {
            KeyCode::Escape => menu.go_back_from_input(),
            KeyCode::ArrowUp => menu.move_focus(-1),
            KeyCode::ArrowLeft => menu.move_horizontal_focus(-1),
            KeyCode::ArrowDown => menu.move_focus(1),
            KeyCode::ArrowRight => menu.move_horizontal_focus(1),
            KeyCode::Tab => menu.move_focus(if modifiers.shift() { -1 } else { 1 }),
            KeyCode::Enter | KeyCode::NumpadEnter => menu.activate_focused(),
            _ if menu.has_focused_field() && !modifiers.shortcut() => {
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
    /// A selected vanilla edit box consumes cancel before the screen handles it.
    fn go_back_from_input(&mut self) {
        if self.screen == super::MenuScreen::Inbox
            && (self.feeds.inbox_state.opened.is_some()
                || self.feeds.inbox_state.delete_pending.is_some()
                || self.feeds.inbox_state.filters)
        {
            self.activate_inbox(super::inbox::Action::Cancel);
            return;
        }
        if self.screen == super::MenuScreen::AddServer && self.field.is_some() {
            self.edit_field(|editor| editor.place_cursor(editor.cursor_byte()));
            self.field = None;
            return;
        }
        self.go_back();
    }
}

#[cfg(test)]
mod gui_scale_drag_tests;
