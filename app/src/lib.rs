pub mod args;
pub mod asset_startup;
mod audio;
mod block_cracks;
mod block_entities;
mod block_selection;
mod block_use;
mod bounded_file;
pub mod camera;
mod environment;
mod first_run;
mod fullscreen;
mod game_mode_capabilities;
mod global_resources;
mod hotbar;
mod hud_tools;
mod install_layout;
mod interaction_authority;
mod item_use;
pub mod lifecycle;
pub mod local_player;
mod local_player_camera_receipt;
#[allow(dead_code, unused_imports, reason = "embedded by the menu module")]
mod local_worlds;
mod melee;
mod menu;
pub mod metrics;
mod mining;
mod modding;
pub mod movement;
mod named_audio;
mod native_dialog;
mod particles;
mod pick_block;
pub mod player_runtime;
mod player_skin;
mod present_mode;
mod render_mode;
mod screen_policy;
pub mod semantic_controls;
pub mod server_camera;
mod server_experiences;
pub mod session_audio;
mod session_cleanup;
pub mod settings_runtime;
#[allow(
    dead_code,
    unused_imports,
    reason = "consumed by the store screens as they land"
)]
mod store;
mod survival_mining;
pub mod ui_runtime;

mod acceptance;
mod app;
mod presentation;
mod presentation_observations;
mod runtime;

pub use app::run;

#[cfg(test)]
mod tests;
