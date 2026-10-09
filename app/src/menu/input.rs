mod cancel;
mod context;
mod death;
mod preview;
mod remapping;
mod server_list;
mod settings_pointer;
mod sounds;

use context::MenuInputContext;
use settings_pointer::{native_release_action, update_slider};

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
        ButtonInput, Entity, KeyCode, Local, MouseButton, Query, Res, ResMut, Resource, Single,
        With,
    },
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window},
};
use ui::{ChatClipboard, ChatEditor, ToastPress, UiPoint};

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

#[derive(Clone, Copy, Debug, Default)]
pub(super) enum MenuInputMode {
    #[default]
    Mouse,
    Touch,
    Keyboard,
    Gamepad,
}

impl MenuInputMode {
    pub(super) fn mouse(self) -> bool {
        matches!(self, Self::Mouse)
    }

    pub(super) fn navigation(self) -> bool {
        matches!(self, Self::Keyboard | Self::Gamepad)
    }

    pub(super) fn gamepad(self) -> bool {
        matches!(self, Self::Gamepad)
    }
}

/// Retains physical button state because menu input clears Bevy's buttons
/// after consumption, and retains slider capture across UI scale relayout.
#[derive(Default)]
pub(crate) struct GuiScaleDrag {
    mouse_cursor: MessageCursor<MouseButtonInput>,
    left_held: bool,
    captured: bool,
    previous_pointer: Option<bevy::prelude::Vec2>,
    pointer_anchor: Option<bevy::prelude::Vec2>,
    touch_slider: Option<(u64, u16)>,
    pending_press: Option<super::MenuAction>,
    touch_press: Option<(u64, super::MenuAction)>,
    preview_touch: Option<u64>,
    server_list_touch: Option<u64>,
    sounds: sounds::MenuPressSounds,
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
            MenuField::RealmCode => &self.realm_membership.code,
            MenuField::Name => &self.name,
            MenuField::Address => &self.address,
            MenuField::Port => &self.port,
            MenuField::WorldName => &self.local_ui.name,
            MenuField::WorldSeed => &self.local_ui.seed,
            MenuField::SkinName => &self.skin_name,
        }
    }

    fn editor_mut(&mut self, field: MenuField) -> &mut ChatEditor {
        match field {
            MenuField::RealmCode => &mut self.realm_membership.code,
            MenuField::Name => &mut self.name,
            MenuField::Address => &mut self.address,
            MenuField::Port => &mut self.port,
            MenuField::WorldName => &mut self.local_ui.name,
            MenuField::WorldSeed => &mut self.local_ui.seed,
            MenuField::SkinName => &mut self.skin_name,
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
        if field == MenuField::SkinName && self.dressing_room.busy {
            return;
        }
        edit(self.editor_mut(field));
        if field == MenuField::RealmCode
            && let Some(state) = &mut self.realm_membership.state
        {
            state.code = self.realm_membership.code.as_str().to_owned();
            state.error = None;
        }
        self.caret_revision = self.caret_revision.wrapping_add(1);
        if field == MenuField::SkinName {
            self.sync_skin_name_draft();
        }
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
    pub(super) fn edit_text(&mut self, text: &str) {
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
        MenuField::RealmCode => MAX_SERVER_ADDRESS_BYTES,
        MenuField::Name => MAX_SERVER_NAME_BYTES,
        MenuField::Address => MAX_SERVER_ADDRESS_BYTES,
        MenuField::Port => MAX_SERVER_PORT_BYTES,
        MenuField::WorldName => MAX_WORLD_NAME_CHARS * 4,
        MenuField::WorldSeed => MAX_SEED_CHARS,
        MenuField::SkinName => launcher::dressing_room::MAX_SKIN_NAME_BYTES,
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_menu_input(
    context: MenuInputContext,
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
    let MenuInputContext {
        player_runtime,
        time,
        mut keyboard_messages,
        mut focus,
        driven,
    } = context;
    let sound_seconds = time.as_deref().map_or(0.0, |time| time.elapsed_secs_f64());
    if let Some(time) = time {
        menu.advance_death_controls(time.delta_secs_f64());
    }
    menu.settings_slider_hovered = None;
    if consent.is_some_and(|consent| consent.0) {
        gui_scale_drag.sounds.clear();
        presentation.cancel_menu_player_preview_input();
        presentation.cancel_menu_server_list_input();
        gui_scale_drag.preview_touch = None;
        menu.settings_slider_pointer = None;
        menu.settings_slider_drag = None;
        gui_scale_drag.pending_press = None;
        gui_scale_drag.touch_press = None;
        gui_scale_drag.touch_slider = None;
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
    menu.refresh_presented_focus(&presentation);
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
    if gui_scale_drag.observe_death(
        runtime.as_deref(),
        &mut menu,
        &mut presentation,
        &mut keyboard_messages,
    ) {
        *modifiers = MenuModifiers::default();
        keys.reset_all();
        mouse_buttons.reset_all();
        crate::camera::release_cursor(&mut cursor);
        return;
    }
    if runtime.as_ref().is_some_and(|runtime| {
        runtime.credits().owns_input()
            || runtime.server_forms().owns_input()
                && (!menu.is_visible() || runtime.server_forms().settings_form_active())
    }) {
        gui_scale_drag.sounds.clear();
        presentation.cancel_menu_player_preview_input();
        presentation.cancel_menu_server_list_input();
        gui_scale_drag.preview_touch = None;
        menu.settings_slider_pointer = None;
        menu.settings_slider_drag = None;
        gui_scale_drag.pending_press = None;
        gui_scale_drag.touch_press = None;
        gui_scale_drag.touch_slider = None;
        gui_scale_drag.captured = false;
        gui_scale_drag.left_held = false;
        keyboard_messages.clear();
        return;
    }
    menu.pressed = None;
    if driven.is_none()
        && (!window.focused || focus.as_ref().is_some_and(|focus| !focus.available()))
    {
        gui_scale_drag.sounds.clear();
        presentation.cancel_menu_player_preview_input();
        presentation.cancel_menu_server_list_input();
        gui_scale_drag.preview_touch = None;
        menu.settings_slider_pointer = None;
        menu.settings_slider_drag = None;
        gui_scale_drag.pending_press = None;
        gui_scale_drag.touch_press = None;
        gui_scale_drag.touch_slider = None;
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
        gui_scale_drag.sounds.clear();
        presentation.cancel_menu_player_preview_input();
        presentation.cancel_menu_server_list_input();
        gui_scale_drag.preview_touch = None;
        menu.settings_slider_pointer = None;
        menu.settings_slider_drag = None;
        gui_scale_drag.pending_press = None;
        gui_scale_drag.touch_press = None;
        gui_scale_drag.touch_slider = None;
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
        } else if cursor.grab_mode == CursorGrabMode::None
            && mouse_buttons.just_pressed(MouseButton::Left)
            && window
                .cursor_position()
                .and_then(|position| UiPoint::new(position.x, position.y).ok())
                .and_then(|point| presentation.toast_press_at(point))
                == Some(ToastPress::JoinRequests)
        {
            // A free cursor pressing the join request toast opens its popup, and nothing beneath;
            // the held press is not a new one for the menu.
            menu.open_join_requests();
            menu.pointer_down = true;
            mouse_buttons.reset(MouseButton::Left);
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

    let gameplay_pending = menu.gameplay_return_pending();
    let respawn_pending = menu.death_loading;
    modifiers.capture_pressed(&keys);
    crate::camera::release_cursor(&mut cursor);
    // The join request popup takes the keyboard from any box focused beneath it.
    if menu.join_request_prompted() {
        menu.field = None;
    }
    let native_settings = presentation.uses_oreui_settings();
    let pointer = window
        .cursor_position()
        .and_then(|position| UiPoint::new(position.x, position.y).ok());
    let cursor_position = window.cursor_position();
    let pointer_moved = cursor_position != gui_scale_drag.previous_pointer;
    gui_scale_drag.previous_pointer = cursor_position;
    menu.hovered = pointer.and_then(|position| presentation.hit_test_menu(position));
    menu.settings_slider_hovered = native_settings
        .then(|| pointer.and_then(|point| presentation.settings_slider_thumb_hit_test(point)))
        .flatten();
    let slider_hovered = menu.settings_slider_hovered;
    let pointer_can_focus = |action| {
        !native_settings
            || match action {
                super::MenuAction::SettingsOption(index, _)
                    if super::settings_options::SETTINGS_OPTIONS
                        .get(usize::from(index))
                        .is_some_and(|option| {
                            matches!(option.kind, super::settings_options::SettingKind::Slider)
                        }) =>
                {
                    slider_hovered == Some(index)
                }
                _ => true,
            }
    };
    let pointer_pressed = mouse_buttons.pressed(MouseButton::Left) || gui_scale_drag.left_held;
    let pointer_just_pressed =
        mouse_buttons.just_pressed(MouseButton::Left) || (pointer_pressed && !menu.pointer_down);
    gui_scale_drag.sounds.observe(
        &menu,
        &presentation,
        pointer,
        pointer_just_pressed,
        &touches,
        sound_seconds,
    );
    let server_list_owns_pointer = server_list::drive(
        &mut presentation,
        &mut menu,
        pointer,
        pointer_pressed,
        pointer_just_pressed,
        &touches,
        &mut gui_scale_drag.server_list_touch,
    );
    let preview_owns_pointer =
        if menu.dialog.is_some() || (menu.is_connecting() && menu.feeds.server_trust.is_some()) {
            gui_scale_drag.preview_touch = None;
            presentation.menu_player_preview_pointer(Some(menu.screen()), pointer, false, false);
            false
        } else {
            preview::drive(
                &mut presentation,
                menu.screen(),
                pointer,
                pointer_pressed,
                pointer_just_pressed,
                &touches,
                &mut gui_scale_drag.preview_touch,
            )
        };
    if preview_owns_pointer {
        menu.hovered = None;
    }
    let pointer_switch = match (gui_scale_drag.pointer_anchor, cursor_position) {
        (Some(anchor), Some(position)) => anchor.distance_squared(position) > 100.0,
        _ => false,
    };
    if gui_scale_drag.pointer_anchor.is_none() || pointer_switch {
        gui_scale_drag.pointer_anchor = cursor_position;
    }
    if pointer_switch || pointer_just_pressed || !wheel.is_empty() {
        menu.input_mode = MenuInputMode::Mouse;
    }
    if (pointer_moved || pointer_just_pressed)
        && let Some(action) = menu.hovered.filter(|action| pointer_can_focus(*action))
    {
        menu.focus_pointer(action);
    }
    if native_settings
        && let Some(point) = pointer
        && let Some(index) = presentation.settings_slider_thumb_hit_test(point)
    {
        menu.hovered = Some(super::MenuAction::SettingsOption(
            index,
            menu.settings_options.get(usize::from(index)),
        ));
    }
    menu.pointer_down = pointer_pressed;
    if !pointer_pressed || menu.screen() != crate::menu::MenuScreen::Settings {
        gui_scale_drag.captured = false;
    }
    if let Some(point) = pointer {
        for (notches, pixels) in wheel {
            presentation.scroll_menu(point, notches, pixels);
        }
    }
    // A scrollbar press or drag scrolls instead of pressing what lies beneath.
    let on_scrollbar = server_list_owns_pointer
        || preview_owns_pointer
        || presentation.drag_menu_scroll(pointer, pointer_pressed)
        || (pointer_just_pressed
            && pointer.is_some_and(|point| presentation.press_menu_scrollbar(point)));
    if on_scrollbar {
        menu.hovered = None;
    }
    let press = |menu: &mut MenuRuntime, action| {
        let action = if native_settings {
            menu.live_settings_action(action)
        } else {
            action
        };
        menu.activate_from_input(action);
    };
    update_slider(
        &mut menu,
        &presentation,
        pointer,
        pointer_pressed,
        pointer_just_pressed,
        native_settings,
    );
    if pointer_just_pressed
        && !on_scrollbar
        && matches!(menu.hovered, Some(super::MenuAction::SettingsScale(_)))
        && pointer.is_some_and(|point| presentation.gui_scale_drag_action(point).is_some())
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
        if native_settings {
            gui_scale_drag.pending_press = menu.settings_slider_drag.is_none().then_some(action);
            menu.pressed = Some(action);
            if pointer_can_focus(action) {
                menu.focus_pointer(action);
            }
        } else {
            press(&mut menu, action);
            caret_press = pointer.zip(action.text_field());
        }
    }
    if pointer_pressed {
        if let Some(action) = gui_scale_drag.pending_press
            && native_release_action(action, menu.hovered).is_some()
        {
            menu.pressed = Some(action);
        }
    } else if let Some(action) = gui_scale_drag.pending_press.take()
        && native_settings
        && let Some(action) = native_release_action(action, menu.hovered)
    {
        press(&mut menu, action);
    }
    if touches.any_just_pressed() {
        menu.input_mode = MenuInputMode::Touch;
    }
    for touch in touches.iter_just_pressed() {
        if server_list_owns_pointer || gui_scale_drag.preview_touch == Some(touch.id()) {
            continue;
        }
        let position = touch.position();
        if let Ok(position) = UiPoint::new(position.x, position.y)
            && let Some(action) = native_settings
                .then(|| presentation.settings_slider_thumb_hit_test(position))
                .flatten()
                .map(|index| {
                    super::MenuAction::SettingsOption(
                        index,
                        menu.settings_options.get(usize::from(index)),
                    )
                })
                .or_else(|| presentation.hit_test_menu(position))
        {
            menu.input_mode = MenuInputMode::Touch;
            if native_settings {
                if !matches!(action, super::MenuAction::SettingsOption(index, _)
                    if super::settings_options::SETTINGS_OPTIONS.get(usize::from(index))
                        .is_some_and(|option| matches!(option.kind, super::settings_options::SettingKind::Slider)))
                    || matches!(action, super::MenuAction::SettingsOption(index, _)
                        if presentation.settings_slider_thumb_contains(index, position))
                {
                    menu.focus_pointer(action);
                }
                menu.pressed = Some(action);
                if let super::MenuAction::SettingsOption(index, _) = action
                    && presentation.settings_slider_thumb_contains(index, position)
                {
                    gui_scale_drag.touch_slider = Some((touch.id(), index));
                    gui_scale_drag.touch_press = None;
                } else {
                    gui_scale_drag.touch_press = Some((touch.id(), action));
                }
            } else if action.text_field().is_some() {
                press(&mut menu, action);
                caret_press = action.text_field().map(|field| (position, field));
            } else {
                gui_scale_drag.touch_press = Some((touch.id(), action));
                menu.pressed = Some(action);
            }
        }
    }
    if let Some((id, index)) = gui_scale_drag.touch_slider {
        if native_settings && let Some(touch) = touches.get_pressed(id) {
            let position = touch.position();
            if let Ok(point) = UiPoint::new(position.x, position.y)
                && let Some(action @ super::MenuAction::SettingsOption(_, value)) =
                    presentation.settings_slider_drag_action(index, point)
                && let Some(fraction) = presentation.settings_slider_drag_fraction(index, point)
            {
                menu.input_mode = MenuInputMode::Touch;
                menu.set_option(index, value);
                menu.settings_slider_pointer = Some(launcher::menu::view::SettingsSliderPointer {
                    option: index,
                    fraction,
                    mouse_input: false,
                });
                menu.pressed = Some(action);
            }
        } else {
            gui_scale_drag.touch_slider = None;
            menu.settings_slider_pointer = None;
        }
    }
    if let Some((id, action)) = gui_scale_drag.touch_press {
        if let Some(touch) = touches.get_released(id) {
            let position = touch.position();
            let hovered = UiPoint::new(position.x, position.y)
                .ok()
                .and_then(|point| presentation.hit_test_menu(point));
            if let Some(action) = native_release_action(action, hovered) {
                press(&mut menu, action);
            }
            gui_scale_drag.touch_press = None;
        } else if let Some(touch) = touches.get_pressed(id) {
            let position = touch.position();
            let hovered = UiPoint::new(position.x, position.y)
                .ok()
                .and_then(|point| presentation.hit_test_menu(point));
            if native_release_action(action, hovered).is_some() {
                menu.pressed = Some(action);
            }
        } else {
            gui_scale_drag.touch_press = None;
        }
    }
    if let Some((point, field)) = caret_press
        && menu.field == Some(field)
        && let Some(byte) = presentation.menu_caret_at(point, field, menu.field_text(field))
    {
        menu.place_caret(byte);
    }
    for gamepad in &gamepads {
        if gamepad.get_just_pressed().next().is_some() {
            menu.input_mode = MenuInputMode::Gamepad;
        }
        if gamepad.just_pressed(GamepadButton::DPadLeft) {
            menu.move_horizontal_focus(-1);
        }
        if gamepad.just_pressed(GamepadButton::DPadRight) {
            menu.move_horizontal_focus(1);
        }
        if gamepad.just_pressed(GamepadButton::DPadUp) {
            menu.move_directional_focus(launcher::menu::view::SettingsFocusAxis::Vertical, -1);
        }
        if gamepad.just_pressed(GamepadButton::DPadDown) {
            menu.move_directional_focus(launcher::menu::view::SettingsFocusAxis::Vertical, 1);
        }
        if gamepad.just_pressed(super::settings_options::gamepad_button(
            &menu.settings_options,
            GamepadButton::South,
        )) {
            sounds::activate_focused(&mut menu, &presentation);
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
        if matches!(
            input.key_code,
            KeyCode::ArrowLeft
                | KeyCode::ArrowRight
                | KeyCode::ArrowUp
                | KeyCode::ArrowDown
                | KeyCode::Tab
        ) {
            menu.input_mode = MenuInputMode::Keyboard;
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
            KeyCode::Backspace if native_settings => menu.go_back_from_input(),
            KeyCode::ArrowUp => {
                menu.move_directional_focus(launcher::menu::view::SettingsFocusAxis::Vertical, -1)
            }
            KeyCode::ArrowLeft => menu.move_horizontal_focus(-1),
            KeyCode::ArrowDown => {
                menu.move_directional_focus(launcher::menu::view::SettingsFocusAxis::Vertical, 1)
            }
            KeyCode::ArrowRight => menu.move_horizontal_focus(1),
            KeyCode::Tab => menu.move_focus(if modifiers.shift() { -1 } else { 1 }),
            KeyCode::Enter | KeyCode::NumpadEnter if menu.field == Some(MenuField::SkinName) => {
                menu.activate(super::MenuAction::DressingRoom(
                    launcher::dressing_room::Action::SaveRename,
                ));
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                sounds::activate_focused(&mut menu, &presentation)
            }
            KeyCode::Space if native_settings => sounds::activate_focused(&mut menu, &presentation),
            _ if menu.has_focused_field() && !modifiers.shortcut() => {
                if let Some(text) = input.text.as_deref() {
                    menu.edit_text(text);
                }
            }
            _ => {}
        }
    }
    if (!menu.is_visible()
        || (!gameplay_pending && menu.gameplay_return_pending())
        || (!respawn_pending && menu.death_loading)
        || menu.pressed == Some(super::MenuAction::ServerTrust(true)))
        && !menu.intents.disconnect
        && let Some(focus) = focus.as_deref_mut()
    {
        focus.authorize_screen_return();
    }
    // The menu owns the pointer and keyboard for this frame. This also keeps
    // the camera's recapture-on-click path from turning a menu click into a
    // gameplay attack or mouse grab.
    let mouse_input = menu.input_mode.mouse();
    if let Some(pointer) = menu.settings_slider_pointer.as_mut() {
        pointer.mouse_input = mouse_input;
    }
    keys.reset_all();
    mouse_buttons.reset_all();
}

#[cfg(test)]
mod gui_scale_drag_tests;

#[cfg(test)]
mod settings_slider_tests;

#[cfg(test)]
mod sign_in_tests;

#[cfg(test)]
mod death_tests;
