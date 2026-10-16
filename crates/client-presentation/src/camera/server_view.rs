//! Server camera instructions, independent animation tracks, and presentation effects.

mod fade;
mod presets;
mod runtime;
mod spline;
mod target;

#[cfg(test)]
mod tutorial_tests;

pub use runtime::{ActorView, ServerCameraSkips, ServerCameraView, ViewContext};
