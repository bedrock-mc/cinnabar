pub mod args;
pub mod asset_startup;
pub mod camera;
mod environment;
mod hotbar;
mod install_layout;
pub mod local_player;
mod menu;
pub mod metrics;
pub mod movement;
mod present_mode;
pub mod semantic_controls;
pub mod server_camera;
mod session_cleanup;
pub mod settings_runtime;
pub mod ui_runtime;

mod acceptance;
mod app;
mod presentation;
mod runtime;

pub use app::run;

#[cfg(test)]
mod tests;
