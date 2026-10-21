//! Menu input, launcher services, desktop settings and Discord presence.
#[cfg(not(target_arch = "wasm32"))]
pub mod discord_presence;
#[cfg(not(target_arch = "wasm32"))]
pub mod fullscreen;
#[cfg(not(target_arch = "wasm32"))]
pub mod local_worlds;
#[cfg(not(target_arch = "wasm32"))]
pub mod menu;
pub mod physical_input;
pub mod player_skin;
#[cfg(not(target_arch = "wasm32"))]
mod plugin;
#[cfg(not(target_arch = "wasm32"))]
pub mod presence_target;
#[cfg(not(target_arch = "wasm32"))]
pub mod screen_policy;
#[cfg(not(target_arch = "wasm32"))]
pub mod settings_runtime;
pub mod ui_resources;
#[cfg(not(target_arch = "wasm32"))]
pub use plugin::ClientShellPlugin;
