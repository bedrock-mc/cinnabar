pub mod args;
pub mod asset_startup;
pub mod camera;
mod environment;
pub mod metrics;
pub mod movement;
pub mod ui_runtime;

mod acceptance;
mod app;
mod runtime;

pub use app::run;

#[cfg(test)]
mod tests;
