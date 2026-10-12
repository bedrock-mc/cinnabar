//! App systems driving client-ui; the app retains system ordering and services.
use client_shell::ui_resources::UiRuntimeResource;
use client_ui::ui_runtime::{UiRuntime, inventory_drag};
pub(crate) mod emotes;
pub mod forms;
pub(crate) mod gameplay_touch;
pub mod interaction;
pub mod presentation;
pub mod scene_stack;
pub mod sign_editor;

pub(crate) use forms::{drive_server_form_input, flush_server_form_network};
pub(crate) use interaction::{
    drive_chat_keyboard_input, drive_chat_ui_actions, drive_inventory_ui_actions,
    drive_world_inventory_keys, flush_chat_network, flush_inventory_network,
};
pub(crate) use sign_editor::drive_sign_editor;

#[cfg(test)]
pub(crate) mod tests;

/// Drains inventory ingress before movement and UI observations at the existing phase.
pub(crate) fn drain_inventory_authority(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    mut runtime: bevy::prelude::ResMut<UiRuntimeResource>,
) {
    runtime.drain_pending_inventory(&mut player_runtime);
}
