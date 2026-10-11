//! Animated actor glint constants shared by armor and player cape textures.

/// Accessibility factors and the actor glint clock, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActorGlint {
    pub time_seconds: f32,
    pub strength: f32,
    pub speed: f32,
}

impl Default for ActorGlint {
    fn default() -> Self {
        Self {
            time_seconds: 0.0,
            strength: 1.0,
            speed: 1.0,
        }
    }
}

impl ActorGlint {
    /// Native actor glint offsets, each wrapping at its own millisecond period.
    pub(crate) fn parameters(self) -> [f32; 3] {
        let time = if self.time_seconds.is_finite() {
            self.time_seconds.max(0.0)
        } else {
            0.0
        };
        let speed = if self.speed.is_finite() {
            self.speed.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let strength = if self.strength.is_finite() {
            self.strength.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let millis = ((time * 1000.0) as i32 as f32 * speed) as i32;
        [
            (millis % 1375) as f32 / -1375.0,
            (millis % 3750) as f32 / 3750.0,
            strength,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_glint_wraps_each_layer_and_scales_speed_after_millisecond_quantization() {
        let factors = ActorGlint {
            time_seconds: 3.75,
            strength: 0.5,
            speed: 1.0,
        };
        assert_eq!(factors.parameters(), [-1000.0 / 1375.0, 0.0, 0.5]);
        let half_speed = ActorGlint {
            time_seconds: 0.0039,
            speed: 0.5,
            ..Default::default()
        };
        assert_eq!(half_speed.parameters(), [-1.0 / 1375.0, 1.0 / 3750.0, 1.0]);
        assert_eq!(
            ActorGlint {
                time_seconds: f32::NAN,
                ..Default::default()
            }
            .parameters(),
            [0.0, 0.0, 1.0]
        );
    }
}
