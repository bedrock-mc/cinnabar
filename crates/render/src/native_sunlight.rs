//! Camera-facing colour terms from the classic Bedrock atmosphere callers.

use std::f32::consts::{PI, TAU};

use bevy::{prelude::Resource, render::extract_resource::ExtractResource};

use crate::celestial;

pub(crate) const SKY_SUN_THRESHOLD: f32 = 0.9;
const FOG_SUN_THRESHOLD: f32 = 0.35;
const SUN_COLOUR_SUBTRACTION: f32 = 0.2;

/// CPU-only view inputs; the fixed-size atmosphere GPU contract is unchanged.
/// Weather fog is the native precipitation-lattice accumulator, not rain level.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct AtmosphereViewInputs {
    pub forward: [f32; 3],
    pub fog_weather_level: f32,
    /// Current simulation rain, not the frame's interpolated rain.
    pub current_rain_level: f32,
}

impl AtmosphereViewInputs {
    /// Vanilla sun intensity; the caller supplies the narrow or broad threshold.
    fn sun_intensity(self, angle: f32, threshold: f32) -> f32 {
        if !angle.is_finite() || !self.forward.into_iter().all(f32::is_finite) {
            return 0.0;
        }
        let half = angle * PI;
        let (sin, cos) = (half + half).sin_cos();
        let admission = if cos < -0.1 {
            0.0
        } else if cos < 0.0 {
            1.0 + cos / 0.1
        } else {
            1.0
        };
        let raw = ((self.forward[1] * cos - self.forward[0] * sin) + 1.0) * 0.5 * admission;
        (raw.clamp(threshold, 1.0) - threshold) / (1.0 - threshold)
            * (1.0 - celestial::unit(self.fog_weather_level))
    }

    pub(crate) fn colour_subtraction(self, angle: f32) -> f32 {
        self.sun_intensity(angle, SKY_SUN_THRESHOLD) * SUN_COLOUR_SUBTRACTION
    }

    /// Vanilla sky colour: precipitation fog first, then thunder.
    pub(crate) fn sky_colour(self, shaded: [f32; 3], angle: f32, thunder: f32) -> [f32; 3] {
        let fog = celestial::unit(self.fog_weather_level);
        if !angle.is_finite() || celestial::unit(self.current_rain_level) <= 0.2 || fog <= 0.0 {
            return shaded;
        }
        let grey = 0.5 * celestial::day_plateau(angle);
        // Thunder is a linear gamma-space transform already applied to `shaded`.
        // Applying it to the target too commutes with the native preceding rain mix.
        let target = celestial::storm_tint([grey; 3], 0.0, thunder);
        let weight = (fog * 4.0).clamp(0.0, 1.0);
        std::array::from_fn(|channel| celestial::lerp(shaded[channel], target[channel], weight))
    }

    pub(crate) fn fog_colour(self, shaded: [f32; 3], angle: f32) -> [f32; 3] {
        let sunrise = celestial::raw_sunrise_band(angle);
        let weight = self.sun_intensity(angle, FOG_SUN_THRESHOLD) * sunrise[3];
        let subtract = self.colour_subtraction(angle);
        std::array::from_fn(|channel| {
            (celestial::lerp(shaded[channel], sunrise[channel], weight) - subtract).clamp(0.0, 1.0)
        })
    }
}

/// Vanilla Overworld fog colour; unlike cloud colour, fog uses full cosf.
pub(crate) fn fog_multipliers(angle: f32) -> [f32; 3] {
    let angle = if angle.is_finite() { angle } else { 0.0 };
    let brightness = ((angle * TAU).cos() * 2.0 + 0.5).clamp(0.0, 1.0);
    [
        brightness * 0.94 + 0.06,
        brightness * 0.94 + 0.06,
        brightness * 0.91 + 0.09,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glare_depends_on_camera_sun_alignment_and_weather_fog_not_rain() {
        let up = AtmosphereViewInputs {
            forward: [0.0, 1.0, 0.0],
            ..Default::default()
        };
        assert_eq!(up.sun_intensity(0.0, SKY_SUN_THRESHOLD), 1.0);
        assert_eq!(
            AtmosphereViewInputs::default().sun_intensity(0.0, SKY_SUN_THRESHOLD),
            0.0
        );
        assert_eq!(up.sun_intensity(0.5, SKY_SUN_THRESHOLD), 0.0);
        assert_eq!(
            AtmosphereViewInputs {
                fog_weather_level: 1.0,
                ..up
            }
            .sun_intensity(0.0, SKY_SUN_THRESHOLD),
            0.0
        );
        assert_eq!(
            AtmosphereViewInputs {
                fog_weather_level: 0.5,
                ..up
            }
            .sun_intensity(0.0, SKY_SUN_THRESHOLD),
            0.5
        );
    }

    #[test]
    fn fog_has_a_separate_blue_night_floor_and_broad_sunrise_threshold() {
        assert_eq!(fog_multipliers(0.5), [0.06, 0.06, 0.09]);
        assert_eq!(fog_multipliers(0.0), [1.0; 3]);
        let towards = AtmosphereViewInputs {
            forward: [-1.0, 0.0, 0.0],
            ..Default::default()
        };
        let away = AtmosphereViewInputs {
            forward: [1.0, 0.0, 0.0],
            ..Default::default()
        };
        assert!(towards.fog_colour([0.5; 3], 0.25)[0] > away.fog_colour([0.5; 3], 0.25)[0]);
        assert_eq!(
            AtmosphereViewInputs {
                forward: [f32::NAN, 0.0, 0.0],
                ..Default::default()
            }
            .colour_subtraction(0.0),
            0.0
        );
    }
}
