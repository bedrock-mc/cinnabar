//! App systems driving client-ui; the app retains system ordering and services.
use client_ui::ui_runtime::{UiRuntime, inventory_drag};
pub(crate) mod emotes;
pub mod forms;
pub(crate) mod gameplay_touch;
pub mod interaction;
pub mod presentation;
pub mod scene_stack;
pub mod sign_editor;
pub(crate) use forms::{drive_server_form_input, flush_server_form_network, typed_text};
pub(crate) use interaction::{
    apply_deferred_inventory_close, drive_chat_keyboard_input, drive_chat_ui_actions,
    drive_inventory_ui_actions, drive_world_inventory_keys, flush_chat_network,
    flush_inventory_network,
};
pub(crate) use sign_editor::drive_sign_editor;

impl client_ui::ui_runtime::presentation::forms::scene_policy::MenuScene
    for crate::menu::MenuRuntime
{
    /// Preserves the current menu visibility without a deferred projection.
    fn is_visible(&self) -> bool {
        self.is_visible()
    }
    /// Supplies the same view formerly read directly by UI.
    fn view(&self) -> launcher::menu::MenuView {
        self.view()
    }
    /// Supplies the current visible screen without changing its transition.
    fn scene(&self) -> Option<launcher::menu::MenuScreen> {
        self.scene()
    }
    /// Preserves the menu's relationship to the connected world.
    fn over_world(&self) -> bool {
        self.over_world()
    }
    /// Preserves the launcher's panorama policy.
    fn uses_panorama(&self) -> bool {
        self.uses_panorama()
    }
}

#[cfg(test)]
mod tests;

/// Drains inventory ingress before movement and UI observations at the existing phase.
pub(crate) fn drain_inventory_authority(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    mut runtime: bevy::prelude::ResMut<UiRuntime>,
) {
    runtime.drain_pending_inventory(&mut player_runtime);
}
