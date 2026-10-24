//! App systems driving client-ui; the app retains system ordering and services.
use client_shell::ui_resources::UiRuntimeResource;
use client_ui::ui_runtime::UiRuntime;
#[cfg(not(target_arch = "wasm32"))]
use client_ui::ui_runtime::inventory_drag;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod data_driven;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod emotes;
#[cfg(not(target_arch = "wasm32"))]
pub mod forms;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod gameplay_touch;
#[cfg(not(target_arch = "wasm32"))]
pub mod interaction;
pub(crate) mod outbound;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod platform_back;
pub mod presentation;
#[cfg(not(target_arch = "wasm32"))]
pub mod scene_stack;
#[cfg(not(target_arch = "wasm32"))]
pub mod sign_editor;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod touch_pointer;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use forms::{drive_server_form_input, flush_server_form_network};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use interaction::{
    drive_chat_keyboard_input, drive_chat_ui_actions, drive_inventory_ui_actions,
    inventory::drive_world_inventory_keys,
};
#[cfg(not(target_arch = "wasm32"))]
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
