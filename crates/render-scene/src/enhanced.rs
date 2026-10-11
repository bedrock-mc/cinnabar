use bevy::{prelude::Component, render::extract_component::ExtractComponent};
/// Per-camera opt-in for Enhanced rendering, with resolved depth for post effects.
#[derive(Component, ExtractComponent, Clone, Copy, Debug, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
pub struct EnhancedRendering {
    pub shadows: bool,
    pub shadow_resolution: u32,
    /// Sun shadow cascades, clamped to `2..=MAX_SHADOW_CASCADES`.
    pub shadow_cascades: u32,
    /// Blocks from the camera covered by the last cascade.
    pub shadow_distance: f32,
    pub bloom: bool,
    pub light_shafts: bool,
    pub waving: bool,
    pub water_reflections: bool,
}

impl Default for EnhancedRendering {
    fn default() -> Self {
        Self {
            shadows: true,
            shadow_resolution: 1024,
            shadow_cascades: 2,
            shadow_distance: 96.0,
            bloom: true,
            light_shafts: true,
            waving: true,
            water_reflections: true,
        }
    }
}

#[cfg(feature = "enhanced-diagnostics")]
impl EnhancedRendering {
    /// Bounds the actual Enhanced passes for hidden development captures.
    #[must_use]
    pub fn bounded_diagnostic() -> Self {
        Self {
            shadow_resolution: MIN_SHADOW_RESOLUTION,
            shadow_distance: 24.0,
            waving: false,
            ..Self::default()
        }
    }
}
pub const MAX_SHADOW_CASCADES: u32 = 3;

/// Smallest shadow target admitted by frame preparation.
pub const MIN_SHADOW_RESOLUTION: u32 = 256;
