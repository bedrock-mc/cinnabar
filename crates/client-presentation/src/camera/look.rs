//! Look input feel: sensitivity curve, analog frame-rate normalization and cinematic smoothing.
//! Curve shape and magnitudes are provisional and need native measurement.

use bevy::prelude::{Resource, Vec2};
use semantic_input::InputMode;

const DEGREES_PER_UNIT_AT_UNIT_CURVE: f32 = 0.15;
/// Multiplier at which the sensitivity slider reads 100%.
const SLIDER_FULL_MULTIPLIER: f32 = 2.0;
const ANALOG_REFERENCE_HZ: f32 = 60.0;
const CINEMATIC_SECONDS_PER_E_FOLD: f32 = 0.06;

/// Radians of rotation per raw look unit for a sensitivity slider fraction in `0..=1`.
#[must_use]
pub fn radians_per_look_unit(slider: f32) -> f32 {
    let slider = if slider.is_finite() {
        slider.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let curve = slider * 0.6 + 0.2;
    (curve * curve * curve * 8.0 * DEGREES_PER_UNIT_AT_UNIT_CURVE).to_radians()
}

/// Factor turning the router's already-multiplied look delta into radians.
///
/// The router scales raw input by the linear multiplier; dividing it back out
/// lets the slider curve, not the multiplier, set the final angle.
#[must_use]
pub fn radians_per_routed_unit(multiplier: f32) -> f32 {
    let multiplier = if multiplier.is_finite() && multiplier > 0.0 {
        multiplier
    } else {
        1.0
    };
    radians_per_look_unit(multiplier / SLIDER_FULL_MULTIPLIER) / multiplier
}

/// Analog sticks report a per-frame delta; scale it so turn rate does not follow frame rate.
#[must_use]
pub fn analog_frame_scale(mode: InputMode, delta_seconds: f32) -> f32 {
    if mode == InputMode::GamePad && delta_seconds.is_finite() && delta_seconds > 0.0 {
        (delta_seconds * ANALOG_REFERENCE_HZ).min(8.0)
    } else {
        1.0
    }
}

/// Scales scoped turns using the held item's damping and the selected input mode's option.
pub fn spyglass_turn_delta(delta: Vec2, scoping: bool, damping: f32) -> Vec2 {
    // Vanilla reduces scoped turns only when the selected damping exceeds the item damping.
    const ITEM_DAMPING: f32 = 0.05;
    if scoping && damping > ITEM_DAMPING {
        delta * (ITEM_DAMPING / damping)
    } else {
        delta
    }
}

/// Exponential low-pass over look deltas that conserves total rotation.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct LookSmoother {
    pending: Vec2,
}

impl LookSmoother {
    pub fn filter(&mut self, delta: Vec2, delta_seconds: f32) -> Vec2 {
        if !delta.is_finite() {
            return Vec2::ZERO;
        }
        self.pending += delta;
        let seconds = if delta_seconds.is_finite() {
            delta_seconds.max(0.0)
        } else {
            0.0
        };
        let release = 1.0 - (-seconds / CINEMATIC_SECONDS_PER_E_FOLD).exp();
        let out = self.pending * release;
        self.pending -= out;
        out
    }

    pub fn reset(&mut self) {
        self.pending = Vec2::ZERO;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_slider_maps_to_the_unit_curve() {
        let per_unit = radians_per_look_unit(0.5);
        assert!((per_unit - 0.15_f32.to_radians()).abs() < 1e-6);
    }

    #[test]
    fn curve_is_monotonic_and_clamped() {
        assert!(radians_per_look_unit(0.25) < radians_per_look_unit(0.75));
        assert_eq!(radians_per_look_unit(2.0), radians_per_look_unit(1.0));
        assert_eq!(radians_per_look_unit(f32::NAN), radians_per_look_unit(0.5));
    }

    #[test]
    fn routed_factor_cancels_the_router_multiplier() {
        let multiplier = 1.0;
        let total = multiplier * radians_per_routed_unit(multiplier);
        assert!((total - radians_per_look_unit(0.5)).abs() < 1e-6);
        assert!(radians_per_routed_unit(f32::NAN).is_finite());
    }

    #[test]
    fn only_gamepad_deltas_scale_with_frame_time() {
        assert_eq!(analog_frame_scale(InputMode::KeyboardMouse, 0.5), 1.0);
        assert!((analog_frame_scale(InputMode::GamePad, 1.0 / 60.0) - 1.0).abs() < 1e-6);
        assert!(analog_frame_scale(InputMode::GamePad, 1.0 / 120.0) < 1.0);
    }

    #[test]
    fn spyglass_damping_scales_both_axes_and_preserves_unscoped_input() {
        let delta = Vec2::new(20.0, -10.0);
        for damping in [0.0, 0.025, 0.05] {
            assert_eq!(spyglass_turn_delta(delta, true, damping), delta);
        }
        assert_eq!(spyglass_turn_delta(delta, true, 0.5), delta * 0.1);
        assert_eq!(spyglass_turn_delta(delta, true, 1.0), delta * 0.05);
        for damping in [0.0, 0.05, 0.5, 1.0] {
            assert_eq!(spyglass_turn_delta(delta, false, damping), delta);
        }
    }

    #[test]
    fn smoother_conserves_rotation() {
        let mut smoother = LookSmoother::default();
        let mut total = smoother.filter(Vec2::new(10.0, -4.0), 0.016);
        for _ in 0..600 {
            total += smoother.filter(Vec2::ZERO, 0.016);
        }
        assert!((total - Vec2::new(10.0, -4.0)).length() < 1e-3);
    }
}
