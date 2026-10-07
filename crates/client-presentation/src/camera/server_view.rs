//! Server camera instructions, independent animation tracks, and presentation effects.

mod fade;
mod presets;
mod runtime;
mod spline;
mod target;

pub use runtime::{ActorView, ServerCameraSkips, ServerCameraView, ViewContext};
