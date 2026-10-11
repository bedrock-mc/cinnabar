//! Menu input, launcher services, desktop settings and Discord presence.
pub mod discord_presence;
pub mod fullscreen;
pub mod local_worlds;
pub mod menu;
pub mod player_skin;
mod plugin;
pub mod presence_target;
pub mod screen_policy;
pub mod settings_runtime;
pub use plugin::ClientShellPlugin;
