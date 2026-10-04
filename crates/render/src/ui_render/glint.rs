//! Settings factors for the existing UI item glint.

use bevy::{prelude::Resource, render::extract_resource::ExtractResource};

/// Normalized accessibility factors, independent of the provisional glint artwork.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, PartialEq)]
pub struct UiGlintSettings {
    pub strength: f32,
    pub speed: f32,
}

impl Default for UiGlintSettings {
    fn default() -> Self {
        Self {
            strength: 1.0,
            speed: 1.0,
        }
    }
}

impl UiGlintSettings {
    /// Applies speed to total time before the shader computes its scrolling phases.
    pub(super) fn animation_seconds(self, elapsed: f32) -> f32 {
        (elapsed * self.speed) % 3600.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_scales_total_time_and_strength_leaves_phase_unchanged() {
        let mut settings = UiGlintSettings::default();
        assert_eq!(settings.animation_seconds(12.0), 12.0);
        settings.speed = 0.5;
        assert_eq!(settings.animation_seconds(12.0), 6.0);
        settings.strength = 0.0;
        assert_eq!(settings.animation_seconds(12.0), 6.0);
        settings.speed = 0.0;
        assert_eq!(settings.animation_seconds(12.0), 0.0);
    }
}
