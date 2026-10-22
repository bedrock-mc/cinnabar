//! Forced death routes retire input owned by the replaced menu.
use super::*;
use bevy::prelude::MessageReader;

impl GuiScaleDrag {
    /// Advances authoritative death/recovery and cancels old menu capture on opening.
    pub(super) fn observe_death(
        &mut self,
        runtime: Option<&client_ui::ui_runtime::UiRuntime>,
        menu: &mut MenuRuntime,
        presentation: &mut UiPresentationRuntime,
        keyboard: &mut MessageReader<KeyboardInput>,
    ) -> bool {
        let Some(runtime) = runtime else {
            return false;
        };
        let lifecycle = (
            runtime.session_id(),
            runtime.local_player_death_generation(),
        );
        if menu.death_lifecycle != Some(lifecycle) {
            menu.note_player_alive();
            menu.death_lifecycle = Some(lifecycle);
        }
        if runtime.local_player_respawn_ready() {
            menu.note_player_alive();
            return false;
        }
        let Some(alive) = runtime.local_player_alive() else {
            return false;
        };
        if alive {
            menu.note_player_alive();
            return false;
        }
        if !menu.open_death_with_rules(runtime.immediate_respawn()) {
            return false;
        }
        menu.death_presentation.hardcore = runtime.gameplay_hud().hardcore();
        presentation.cancel_menu_player_preview_input();
        presentation.cancel_menu_server_list_input();
        presentation.drag_menu_scroll(None, false);
        menu.settings.slider_pointer = None;
        menu.settings_input.slider_drag = None;
        menu.settings_input.shader_slider_drag = None;
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
