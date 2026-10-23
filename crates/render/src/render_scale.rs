//! Output dimensions retained when a view renders at a smaller internal resolution.
use bevy::prelude::{Component, UVec2};

/// Marks a reduced render view without changing its window, layout, or input coordinates.
#[derive(Component, Clone, Copy, Debug)]
pub struct RenderScaleOutput(pub UVec2);

/// Selects RCAS sharpness in stops; `None` resolves EASU without sharpening.
#[derive(bevy::prelude::Resource)]
pub struct RenderScaleSharpen(pub Option<f32>);
