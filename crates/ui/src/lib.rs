//! Vendor-independent user-interface primitives.

mod action;
mod geometry;
mod settings;

pub use action::{PointerPhase, UiAction, UiLimits};
pub use geometry::{DpiScale, GeometryError, SafeArea, UiPoint, UiRect, UiScale};
pub use settings::{CURRENT_SETTINGS_SCHEMA, GameplaySettings, UserSettings, VideoSettings};
