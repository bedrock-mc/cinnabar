pub mod args;
pub mod asset_startup;
pub mod camera;
mod environment;
pub mod local_player;
pub mod metrics;
pub mod movement;
mod present_mode;
pub mod semantic_controls;
pub mod settings_runtime;
pub mod ui_runtime;

mod acceptance;
mod app;
mod runtime;

pub use app::run;

#[cfg(test)]
mod tests;
