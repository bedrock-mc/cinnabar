//! Ordinary transparent-liquid alpha distance, separate from fog and cloud fade.

use super::AtmosphereFrame;

const HALF_DISTANCE_MIN: f32 = 48.0;
const HALF_DISTANCE_MAX: f32 = 160.0;
const BASE_FOG_OFFSET: f32 = 40.0;
const BASE_FOG_MARGIN_MIN: f32 = 4.0;
const BASE_FOG_MARGIN_MAX: f32 = 96.0;
// Vanilla distance magnitude: 13.856406211853027, retained at its exact f32 bits.
const INTERIOR_INSET: f32 = f32::from_bits(0x415d_b3d7);
const ALPHA_DISTANCE_INSET: f32 = 7.0;

/// The camera derives its distance scalar from adjusted render distance
/// and subtracts seven for FogAndDistanceControl.w. The input has already
/// passed the render-distance adjustment and minimum of forty.
pub(super) fn alpha_distance_blocks(adjusted_render_distance: f32) -> Option<f32> {
    if !adjusted_render_distance.is_finite() || adjusted_render_distance < BASE_FOG_OFFSET {
        return None;
    }
    let half = (adjusted_render_distance * 0.5).clamp(HALF_DISTANCE_MIN, HALF_DISTANCE_MAX);
    let margin = ((adjusted_render_distance - BASE_FOG_OFFSET) * 0.5)
        .clamp(BASE_FOG_MARGIN_MIN, BASE_FOG_MARGIN_MAX);
    let base_start = adjusted_render_distance - margin;
    Some(half.min(base_start - INTERIOR_INSET) - ALPHA_DISTANCE_INSET)
}

impl AtmosphereFrame {
    /// Sets the ordinary above-water alpha distance from adjusted render distance
    /// in blocks. Invalid/unknown input disables distance adjustment, not fog.
    ///
    /// Incomplete: the native underwater/no-FrameBuilder branch uses a distinct
    /// half-distance and minimum. Its admission is not represented by this API.
    #[must_use]
    pub fn with_liquid_render_distance(mut self, adjusted_blocks: f32) -> Self {
        self.liquid_distance.x = alpha_distance_blocks(adjusted_blocks).unwrap_or_default();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{AtmosphereFrame, alpha_distance_blocks};

    #[test]
    fn current_above_water_distance_matches_native_clamp_branches() {
        for (render_distance, expected) in [
            (40.0, 15.143594),
            (64.0, 31.143593),
            (96.0, 41.0),
            (128.0, 57.0),
            (240.0, 113.0),
            (256.0, 121.0),
            (512.0, 153.0),
            (f32::MAX, 153.0),
        ] {
            let actual = alpha_distance_blocks(render_distance).unwrap();
            assert!(
                (actual - expected).abs() < 2e-6,
                "{render_distance}: {actual}"
            );
        }
    }

    #[test]
    fn malformed_or_unadjusted_short_distance_is_not_a_shader_divisor() {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 39.0] {
            assert_eq!(alpha_distance_blocks(invalid), None);
            assert_eq!(
                AtmosphereFrame::default()
                    .with_liquid_render_distance(invalid)
                    .liquid_distance
                    .x,
                0.0
            );
        }
    }

    #[test]
    fn liquid_distance_is_append_only_and_independent_of_weather_fog_and_clouds() {
        let baseline = AtmosphereFrame::from_bedrock_time(18_000.0, 0.7, 0.3)
            .with_cloud_fade_distance(768.0)
            .with_cloud_renderer_ticks(999.0);
        let applied = baseline.with_liquid_render_distance(128.0);
        let prefix = std::mem::offset_of!(AtmosphereFrame, liquid_distance);
        assert_eq!(
            &bytemuck::bytes_of(&baseline)[..prefix],
            &bytemuck::bytes_of(&applied)[..prefix]
        );
        assert_eq!(applied.liquid_distance.to_array(), [57.0, 0.0, 0.0, 0.0]);
        assert_eq!(AtmosphereFrame::default().liquid_distance.x, 121.0);
    }
}
