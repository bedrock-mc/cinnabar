//! Shared unlit material parameters. UV.x drives gradients; line distance drives dashes.
//!
//! The host clock starts at zero on each connection, advances while gameplay is paused,
//! and wraps every `ANIMATION_WRAP_SECONDS`. Rates are whole cycles per wrap, so the
//! presets remain continuous at the boundary. Periods are rounded to that rate grid.

/// Fixed parameter count for built-in and custom materials.
pub const PARAMS: usize = 16;
/// Session animation phase is elapsed real seconds divided by this period, modulo one.
pub const ANIMATION_WRAP_SECONDS: f64 = 4096.0;

/// A reusable shared-material definition; combine presets by editing its parameter block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset(pub [f32; PARAMS]);
impl Default for Preset {
    /// Solid vertex colour multiplied by instance tint, without emission.
    fn default() -> Self {
        let mut values = [0.; PARAMS];
        values[7] = 1.;
        values[9] = 1.;
        Self(values)
    }
}
impl Preset {
    /// Tints vertex colours with a hue wheel once per requested period.
    pub fn rainbow(period_seconds: f32) -> Self {
        let mut preset = Self::default();
        preset.0[4] = cycles(period_seconds);
        preset
    }
    /// Modulates coverage between `minimum` and one without changing geometry.
    pub fn pulse(period_seconds: f32, minimum: f32) -> Self {
        let mut preset = Self::default();
        preset.0[6] = cycles(period_seconds);
        preset.0[7] = minimum.clamp(0., 1.);
        preset
    }
    /// Multiplies vertex RGB by a gradient from white to `end` across UV.x.
    pub fn gradient(end: [f32; 3]) -> Self {
        let mut preset = Self::default();
        preset.0[..3].copy_from_slice(&end);
        preset.0[3] = 1.;
        preset
    }
    /// Repeats a dash in local line units; positive periods scroll one dash per cycle.
    pub fn dashed(length: f32, duty: f32, period_seconds: f32) -> Self {
        let mut preset = Self::default();
        preset.0[8] = length;
        preset.0[9] = duty.clamp(0., 1.);
        preset.0[10] = cycles(period_seconds);
        preset
    }
    /// Adds bounded world emission. Display-space halos require the separate mask path.
    pub fn emitting(mut self, intensity: f32) -> Self {
        self.0[11] = intensity.clamp(0., crate::MAX_MATERIAL_EMISSION);
        self
    }
}
/// Quantizes a finite positive period to whole cycles per wrapped session phase.
fn cycles(period: f32) -> f32 {
    if period.is_finite() && period > 0. {
        (ANIMATION_WRAP_SECONDS as f32 / period)
            .round()
            .clamp(1., 65536.)
    } else {
        0.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_encode_composable_fixed_blocks_and_wrapping_rates() {
        for preset in [
            Preset::default(),
            Preset::rainbow(3.),
            Preset::pulse(2., 0.2),
            Preset::gradient([1., 0., 0.]),
            Preset::dashed(0.5, 0.5, 4.),
        ] {
            assert_eq!(preset.0.len(), PARAMS);
            assert!(preset.0.iter().all(|v| v.is_finite()));
        }
        let rainbow = Preset::rainbow(3.);
        assert_eq!(rainbow.0[4].fract(), 0.);
        assert_eq!(Preset::pulse(2., 0.2).0[6].fract(), 0.);
        assert_eq!(Preset::dashed(1., 0.5, 3.).0[10].fract(), 0.);
    }
}
