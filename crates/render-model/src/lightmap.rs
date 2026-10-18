//! Classic RGB lightmap.

/// Raw light values and byte-truncated values shared by all world materials.
#[derive(Clone, Copy, Debug, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct LightmapUniform {
    pub raw: [[f32; 4]; 256],
    pub native: [[f32; 4]; 256],
}

impl LightmapUniform {
    /// Keeps raw consumers unchanged and quantizes each native RGB entry once per upload.
    pub fn new(raw: [[f32; 4]; 256]) -> Self {
        let native = raw.map(|rgba| {
            let mut quantized = rgba;
            for channel in &mut quantized[..3] {
                *channel = (channel.clamp(0.0, 1.0) * 255.0).floor() / 255.0;
            }
            quantized
        });
        Self { raw, native }
    }
}

/// Inputs to the classic light texture builder, before material color conversion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightmapInputs {
    /// Present only for the optional Java world preset.
    pub java: Option<crate::java_world::JavaLightmapInputs>,
    pub ramp: [f32; 16],
    pub sky_darken: f32,
    pub sunrise: [f32; 4],
    pub lightning: bool,
    pub brightness: f32,
    pub night_vision: f32,
    pub darkness: f32,
    pub darkness_pulse: f32,
    pub ambient_adjustment: bool,
    pub dimension_tint: Option<[f32; 3]>,
}

impl Default for LightmapInputs {
    fn default() -> Self {
        Self {
            java: None,
            // The ordinary zero-ambient light ramp follows this curve.
            // This curve matches its table apart from a few f32 ULPs.
            ramp: std::array::from_fn(|level| level as f32 / (60 - 3 * level) as f32),
            sky_darken: 1.0,
            sunrise: [0.0; 4],
            lightning: false,
            brightness: 0.0,
            night_vision: 0.0,
            darkness: 0.0,
            darkness_pulse: 0.0,
            ambient_adjustment: false,
            dimension_tint: None,
        }
    }
}

impl LightmapInputs {
    /// Builds all sky/block pairs once for every ordinary world material.
    pub fn build(self) -> [[f32; 4]; 256] {
        if let Some(java) = self.java {
            return java.build();
        }
        std::array::from_fn(|index| {
            let rgb = self.colour(self.ramp[index & 15], self.ramp[index >> 4]);
            [rgb[0], rgb[1], rgb[2], 1.0]
        })
    }

    /// Composes colored sky and block light, then applies effects and gamma in vanilla order.
    pub fn colour(self, block: f32, sky: f32) -> [f32; 3] {
        let b = 1.6 * block;
        let block = [
            b,
            ((0.6 * b + 0.4) * 0.6 + 0.4) * b,
            (0.6 * b * b + 0.4) * b,
        ];
        let mut color = if let Some(tint) = self.dimension_tint {
            let offsets = [0.22, 0.28, 0.25];
            std::array::from_fn(|i| 0.75 * (sky * tint[i] + block[i]) + offsets[i])
        } else {
            let a = 0.3 * self.sunrise[3];
            let q = 0.95 * self.sky_darken + 0.05;
            std::array::from_fn(|i| {
                let s = if self.lightning {
                    sky
                } else {
                    sky * (self.sunrise[i] * a + q * (1.0 - a))
                };
                block[i]
                    + s * if i < 2 {
                        0.65 * self.sky_darken + 0.35
                    } else {
                        1.0
                    }
            })
        };
        if self.ambient_adjustment {
            color = color.map(|c| 0.96 * c + 0.03);
        }
        let maximum = color.into_iter().fold(0.0_f32, f32::max);
        if maximum > 0.0 {
            color = color.map(|c| c + (c / maximum - c) * self.night_vision);
        }
        let gamma = (self.brightness - self.darkness).max(0.0);
        color.map(|c| {
            let c = (c - self.darkness_pulse).clamp(0.0, 1.0);
            let complement = 1.0 - c;
            let squared = complement * complement;
            let c = c + (1.0 - squared * squared - c) * gamma;
            let c = if self.ambient_adjustment {
                0.96 * c + 0.03
            } else {
                c
            };
            c.clamp(0.0, 1.0)
        })
    }
}

/// Interpolates the darkness strength and its current classic pulse.
pub fn darkness_pulse(tick: f32, partial: f32, previous: f32, current: f32, amplitude: f32) -> f32 {
    let strength = previous + (current - previous) * partial;
    (((tick - partial) * 0.078_539_82).cos() * amplitude * strength).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Checks raw and prequantized uploads across byte boundaries and all environment builders.
    #[test]
    fn lightmap_upload_preserves_raw_values_and_truncates_only_native_rgb() {
        let mut cases = vec![None];
        for dimension_tint in [None, Some([0.22, 0.28, 0.25]), Some([0.7, 0.1, 0.4])] {
            for (sky_darken, brightness, night_vision, darkness, lightning) in [
                (0.0, 0.0, 0.0, 0.0, false),
                (1.0, 1.0, 0.0, 0.0, false),
                (0.4, 0.5, 0.7, 0.25, true),
            ] {
                cases.push(Some(LightmapInputs {
                    dimension_tint,
                    sky_darken,
                    brightness,
                    night_vision,
                    darkness,
                    lightning,
                    ..Default::default()
                }));
            }
        }
        for inputs in cases {
            let raw = inputs.map_or([[1.0; 4]; 256], LightmapInputs::build);
            let upload = LightmapUniform::new(raw);
            assert_eq!(upload.raw, raw);
            for (entry, native) in raw.into_iter().zip(upload.native) {
                for channel in 0..3 {
                    assert_eq!(
                        native[channel],
                        (entry[channel].clamp(0.0, 1.0) * 255.0).floor() / 255.0
                    );
                }
                assert_eq!(native[3], entry[3]);
            }
        }
        let raw: [[f32; 4]; 256] = std::array::from_fn(|byte| {
            let boundary = byte as f32 / 255.0;
            [boundary.next_down(), boundary, boundary.next_up(), 0.25]
        });
        let upload = LightmapUniform::new(raw);
        assert_eq!(upload.raw, raw);
        for (raw, native) in raw.into_iter().zip(upload.native) {
            for channel in 0..3 {
                assert_eq!(
                    native[channel],
                    (raw[channel].clamp(0.0, 1.0) * 255.0).floor() / 255.0
                );
            }
            assert_eq!(native[3], 0.25);
        }
    }
}
