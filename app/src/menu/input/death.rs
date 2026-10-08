//! Forced death routes retire input owned by the replaced menu.
use super::*;

impl GuiScaleDrag {
    /// Advances authoritative death/recovery and cancels old menu capture on opening.
    pub(super) fn observe_death(
        &mut self,
        runtime: Option<&client_ui::ui_runtime::UiRuntime>,
        menu: &mut MenuRuntime,
        presentation: &mut UiPresentationRuntime,
        keyboard: &mut MessageReader<KeyboardInput>,
    ) -> bool {
        let Some(health) = runtime.and_then(|runtime| runtime.hud().health()) else {
            return false;
        };
        if health.current() != 0 {
            menu.note_player_alive();
            return false;
        }
        if !menu.open_death_with_rules(runtime.is_some_and(|runtime| runtime.immediate_respawn())) {
            return false;
        }
        presentation.cancel_menu_player_preview_input();
        presentation.cancel_menu_server_list_input();
        presentation.drag_menu_scroll(None, false);
        menu.settings_slider_pointer = None;
        menu.settings_slider_drag = None;
        menu.pressed = None;
        menu.hovered = None;
        menu.pointer_down = false;
        *self = Self {
            mouse_cursor: std::mem::take(&mut self.mouse_cursor),
            ..Default::default()
        };
        keyboard.clear();
        true
    }
}
