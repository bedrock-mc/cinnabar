//! Routes cancel input through selected controls before leaving the menu.

use super::*;

impl GuiScaleDrag {
    /// Releases gestures when Back replaces the screen beneath the pointer.
    pub(super) fn cancel_pointer(
        &mut self,
        menu: &mut MenuRuntime,
        presentation: &mut UiPresentationRuntime,
    ) {
        presentation.cancel_menu_player_preview_input();
        presentation.cancel_menu_server_list_input();
        self.captured = false;
        self.left_held = false;
        self.pending_press = None;
        self.touch_press = None;
        self.touch_slider = None;
        self.preview_touch = None;
        self.server_list_touch = None;
        menu.pointer_down = false;
        menu.pressed = None;
        menu.settings.slider_pointer = None;
        menu.settings_input.slider_drag = None;
        menu.settings_input.shader_slider_drag = None;
    }
}

impl MenuRuntime {
    /// A selected vanilla edit box consumes cancel before the screen handles it.
    pub(super) fn go_back_from_input(&mut self) {
        if self.sign_in_focus().is_some() || self.join_request_prompted() {
            self.go_back();
            return;
        }
        if self.clear_settings_slider_selection() {
            return;
        }
        if self.screen == launcher::menu::MenuScreen::Inbox
            && (self.feeds.inbox_state.opened.is_some()
                || self.feeds.inbox_state.delete_pending.is_some()
                || self.feeds.inbox_state.filters)
        {
            self.activate_inbox(launcher::menu::inbox::Action::Cancel);
            return;
        }
        if self.screen == launcher::menu::MenuScreen::AddServer && self.field.is_some() {
            self.edit_field(|editor| editor.place_cursor(editor.cursor_byte()));
            self.field = None;
            return;
        }
        self.go_back();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::*;
    use launcher::menu::{MenuAction, MenuScreen};

    #[test]
    fn mouse_back_returns_one_menu_level_without_a_keyboard_binding() {
        let mut menu = MenuRuntime::new(true, 2, "Player".into());
        menu.show_world();
        menu.open_pause();
        menu.activate(MenuAction::PauseSettings);
        assert_eq!(menu.screen(), MenuScreen::Settings);
        let mut app = App::new();
        app.add_message::<KeyboardInput>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<Touches>()
            .init_resource::<MenuClipboard>()
            .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
                1,
            )))
            .insert_resource(menu)
            .insert_resource(UiPresentationResource(
                UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap(),
            ))
            .add_systems(Update, drive_menu_input);
        app.world_mut().spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Back);
        app.update();
        let menu = app.world().resource::<MenuRuntime>();
        assert_eq!(menu.screen(), MenuScreen::Pause);
        assert!(
            menu.is_visible(),
            "one Back press does not also dismiss pause"
        );
        app.update();
        assert!(app.world().resource::<MenuRuntime>().is_visible());
    }
}
