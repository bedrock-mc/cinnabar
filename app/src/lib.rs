pub mod args;
pub mod asset_startup;
pub mod camera;
mod environment;
pub mod local_player;
pub mod metrics;
pub mod movement;
pub mod semantic_controls;
pub mod settings_runtime;
pub mod ui_runtime;

mod acceptance;
mod app;
mod presentation;
mod runtime;

pub use app::run;

#[cfg(test)]
mod tests;
