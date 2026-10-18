//! Java world presentation math, independent of the client and graphics backend.

mod environment;
pub use environment::{JavaCameraInputs, JavaVisualState, JavaWorldFrame};

/// Evaluated light attributes and player effects for a single world frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JavaLightmapInputs {
    pub ambient: [f32; 3],
    pub sky_colour: [f32; 3],
    pub sky_factor: f32,
    pub block_tint: [f32; 3],
    pub block_factor: f32,
    pub night_vision_colour: [f32; 3],
    pub night_vision: f32,
    pub brightness: f32,
    pub darkness: f32,
    pub boss_darken: f32,
}

impl JavaLightmapInputs {
    /// Produces the quantized RGB light texture in block-major, then sky order.
    pub fn build(self) -> [[f32; 4]; 256] {
        std::array::from_fn(|index| {
            let rgb = self.colour((index & 15) as u8, (index >> 4) as u8);
            [rgb[0], rgb[1], rgb[2], 1.0]
        })
    }

    /// Preserves light hue while applying the player's brightness to its maximum channel.
    pub fn colour(self, block: u8, sky: u8) -> [f32; 3] {
        let level = f32::from(block.min(15)) / 15.0;
        let block_strength = light_ramp(level) * self.block_factor;
        let sky_strength = light_ramp(f32::from(sky.min(15)) / 15.0) * self.sky_factor;
        let white = 0.9 * (2.0 * level - 1.0).powi(2);
        let colour: [f32; 3] = std::array::from_fn(|channel| {
            let ambient =
                self.ambient[channel].max(self.night_vision_colour[channel] * self.night_vision);
            let block_colour = self.block_tint[channel] * (1.0 - white) + white;
            let lit =
                ambient + self.sky_colour[channel] * sky_strength + block_colour * block_strength;
            let boss_loss = if channel == 0 { 0.3 } else { 0.4 };
            (lit * (1.0 - self.boss_darken * boss_loss) - self.darkness).clamp(0.0, 1.0)
        });
        let maximum = colour.into_iter().fold(0.0_f32, f32::max);
        let gain = if maximum > 0.0 {
            let raised = 1.0 - (1.0 - maximum).powi(4);
            1.0 + (raised / maximum - 1.0) * self.brightness
        } else {
            1.0
        };
        colour.map(|channel| (channel * gain * 255.0).round().clamp(0.0, 255.0) / 255.0)
    }
}

/// Converts a normalized light level to its intensity before environmental tinting.
fn light_ramp(level: f32) -> f32 {
    level / (4.0 - 3.0 * level)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An unlit synthetic environment isolates the hue-preserving brightness operation.
    fn unlit() -> JavaLightmapInputs {
        JavaLightmapInputs {
            ambient: [0.0; 3],
            sky_colour: [1.0; 3],
            sky_factor: 1.0,
            block_tint: [1.0, 0.85, 0.55],
            block_factor: 1.4,
            night_vision_colour: [0.6; 3],
            night_vision: 0.0,
            brightness: 0.0,
            darkness: 0.0,
            boss_darken: 0.0,
        }
    }

    #[test]
    fn zero_light_stays_finite_and_sky_light_is_independent_of_block_tint() {
        let input = unlit();
        assert_eq!(input.colour(0, 0), [0.0; 3]);
        assert_eq!(input.colour(0, 15), [1.0; 3]);
        assert_eq!(input.colour(15, 15), [1.0; 3]);
        assert_eq!(
            JavaLightmapInputs {
                brightness: 1.0,
                ..input
            }
            .colour(0, 0),
            [0.0; 3]
        );
        let block = input.colour(8, 0);
        assert!(block[0] > block[1] && block[1] > block[2]);
    }

    #[test]
    fn brightness_preserves_colour_ratios_with_only_unorm_rounding_error() {
        let input = JavaLightmapInputs {
            ambient: [0.1, 0.2, 0.3],
            ..unlit()
        };
        let brighter = JavaLightmapInputs {
            brightness: 1.0,
            ..input
        }
        .colour(0, 0);
        assert!(brighter[0] > 0.1 && brighter[2] > 0.3);
        assert!((brighter[0] * 3.0 - brighter[2]).abs() <= 2.0 / 255.0);
        assert!((brighter[1] * 1.5 - brighter[2]).abs() <= 2.0 / 255.0);
    }

    #[test]
    fn night_vision_and_darkness_affect_every_light_pair() {
        let input = unlit();
        let night = JavaLightmapInputs {
            night_vision: 1.0,
            ..input
        };
        let dark = JavaLightmapInputs {
            darkness: 0.5,
            ..input
        };
        assert_eq!(night.colour(0, 0), [0.6; 3]);
        for (ordinary, darker) in input.build().into_iter().zip(dark.build()) {
            for channel in 0..3 {
                assert!(darker[channel] <= ordinary[channel]);
            }
            assert_eq!(ordinary[3], 1.0);
        }
    }
}
