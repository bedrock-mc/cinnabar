use serde::{Deserialize, Serialize};

use crate::FogDistanceMode;

/// Initial fog and timing retained from a pack's `transition_fog` record.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FogTransition {
    pub mode: FogDistanceMode,
    pub start_bits: u32,
    pub end_bits: u32,
    pub rgb8: u32,
    pub min_percent_bits: u32,
    pub mid_seconds_bits: u32,
    pub mid_percent_bits: u32,
    pub max_seconds_bits: u32,
}

impl FogTransition {
    /// Resolves pack units into the fields blended by the fog layer evaluator.
    pub(crate) fn resolve(self, render_distance: f32) -> [f32; 9] {
        let scale = match self.mode {
            FogDistanceMode::Fixed => 1.0,
            FogDistanceMode::RenderRelative => render_distance,
        };
        let rgb = [16, 8, 0].map(|shift| ((self.rgb8 >> shift) & 255) as f32 / 255.0);
        [
            f32::from_bits(self.start_bits) * scale,
            f32::from_bits(self.end_bits) * scale,
            rgb[0],
            rgb[1],
            rgb[2],
            f32::from_bits(self.min_percent_bits),
            f32::from_bits(self.mid_seconds_bits),
            f32::from_bits(self.mid_percent_bits),
            f32::from_bits(self.max_seconds_bits),
        ]
    }

    /// Checks the distance, color and ordered timing intervals before carrier admission.
    #[must_use]
    pub fn is_valid(self) -> bool {
        let [start, end, min, mid_time, mid, max_time] = [
            self.start_bits,
            self.end_bits,
            self.min_percent_bits,
            self.mid_seconds_bits,
            self.mid_percent_bits,
            self.max_seconds_bits,
        ]
        .map(f32::from_bits);
        [start, end, min, mid_time, mid, max_time]
            .into_iter()
            .all(f32::is_finite)
            && end >= start
            && (0.0..=1.0).contains(&min)
            && (0.0..=1.0).contains(&mid)
            && max_time >= 0.0
            && mid_time >= 0.0
            && (mid_time == 0.0 || max_time > mid_time)
            && self.rgb8 <= 0x00ff_ffff
    }
}

/// Blends the resolved initial fog into the target using the current client's two-stage timeline.
pub(crate) fn apply(
    values: [f32; 9],
    target: crate::ResolvedFog,
    seconds: f32,
) -> crate::ResolvedFog {
    let [start, end, r, g, b, min, mid, percent, max] = values;
    if seconds >= max {
        return target;
    }
    let amount = if mid == 0.0 {
        (seconds / max).clamp(0.0, 1.0)
    } else {
        let first = (seconds / mid).clamp(0.0, 1.0);
        let second = ((seconds - mid) / (max - mid)).clamp(0.0, 1.0);
        percent * (first - second) + second
    }
    .max(min);
    let initial = [r, g, b];
    crate::ResolvedFog {
        start: start + (target.start - start) * amount,
        end: end + (target.end - end) * amount,
        rgb: std::array::from_fn(|i| target.rgb[i] * amount + initial[i] * (1.0 - amount)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A profile with distinct initial and final colors exposes interpolation of every field.
    fn transition() -> FogTransition {
        FogTransition {
            mode: FogDistanceMode::Fixed,
            start_bits: 0.0_f32.to_bits(),
            end_bits: 0.01_f32.to_bits(),
            rgb8: 0,
            min_percent_bits: 0.25_f32.to_bits(),
            mid_seconds_bits: 5.0_f32.to_bits(),
            mid_percent_bits: 0.6_f32.to_bits(),
            max_seconds_bits: 30.0_f32.to_bits(),
        }
    }

    #[test]
    fn current_timeline_clamps_to_minimum_instead_of_lerping_from_it() {
        let target = crate::ResolvedFog {
            start: 4.0,
            end: 60.0,
            rgb: [1.0; 3],
        };
        for (seconds, expected) in [
            (0.0, 0.25),
            (1.0, 0.25),
            (2.5, 0.3),
            (5.0, 0.6),
            (17.5, 0.8),
            (30.0, 1.0),
        ] {
            let fog = apply(transition().resolve(128.0), target, seconds);
            assert!((fog.start - 4.0 * expected).abs() < 0.00001);
            assert!((fog.end - (0.01 + 59.99 * expected)).abs() < 0.00001);
            assert!((fog.rgb[0] - expected).abs() < 0.00001);
        }
    }

    #[test]
    fn minimum_can_exceed_midpoint_but_negative_duration_is_invalid() {
        let mut value = transition();
        value.min_percent_bits = 0.8_f32.to_bits();
        assert!(value.is_valid());
        value.mid_seconds_bits = 0;
        value.max_seconds_bits = (-1.0_f32).to_bits();
        assert!(!value.is_valid());
    }

    #[test]
    fn zero_midpoint_uses_one_ramp_and_zero_duration_returns_target() {
        let target = crate::ResolvedFog {
            start: 4.0,
            end: 60.0,
            rgb: [1.0; 3],
        };
        let mut transition = transition();
        transition.mid_seconds_bits = 0;
        assert_eq!(apply(transition.resolve(128.0), target, 15.0).start, 2.0);
        transition.max_seconds_bits = 0;
        assert_eq!(apply(transition.resolve(128.0), target, 0.0), target);
    }
}
