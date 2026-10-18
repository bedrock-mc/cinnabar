//! Bounded local damage-camera styling, independent of gameplay and camera aim.

/// Largest supported multiplier for a module's damage shake.
pub const MAX_HURT_CAMERA_INTENSITY: f32 = 2.0;

/// A validated temporary presentation override; absence restores the player's camera settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HurtCameraStyle {
    intensity: f32,
    old_tilt: bool,
}

impl HurtCameraStyle {
    /// Accepts finite intensity within the supported range; zero disables damage shake.
    pub fn new(intensity: f32, old_tilt: bool) -> Option<Self> {
        (intensity.is_finite() && (0.0..=MAX_HURT_CAMERA_INTENSITY).contains(&intensity)).then_some(
            Self {
                intensity,
                old_tilt,
            },
        )
    }

    /// Multiplier for damage shake only; death rotation and ordinary view bob are separate.
    pub const fn intensity(self) -> f32 {
        self.intensity
    }

    /// Whether damage uses the fixed old tilt axis rather than its observed direction.
    pub const fn old_tilt(self) -> bool {
        self.old_tilt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Overrides cannot inject non-finite transforms or silently cap the supported double shake.
    #[test]
    fn damage_style_admits_the_complete_range_and_rejects_invalid_values() {
        for intensity in [0.0, 0.5, 1.0, MAX_HURT_CAMERA_INTENSITY] {
            for old_tilt in [false, true] {
                let style = HurtCameraStyle::new(intensity, old_tilt).unwrap();
                assert_eq!(style.intensity(), intensity);
                assert_eq!(style.old_tilt(), old_tilt);
            }
        }
        for intensity in [
            -0.01,
            MAX_HURT_CAMERA_INTENSITY + 0.01,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            assert!(HurtCameraStyle::new(intensity, false).is_none());
        }
    }
}
