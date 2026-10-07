//! Look input feel: vanilla's mouse look curve, analog look scaling, frame-rate normalization and
//! cinematic smoothing. The analog curve's shape and magnitudes are provisional.

use bevy::prelude::{Resource, Vec2};
use semantic_input::{DEFAULT_MOUSE_SENSITIVITY, InputMode};

/// Vanilla's game sensitivity before the sensitivity option first reports a change.
const DEFAULT_GAME_SENSITIVITY: f32 = 0.628;
/// Writes closer than this to a vanilla float option's value are ignored.
const OPTION_EPSILON: f32 = 0.001;
const ANALOG_DEGREES_PER_UNIT_AT_UNIT_CURVE: f32 = 0.15;
/// Multiplier at which the analog sensitivity slider reads 100%.
const SLIDER_FULL_MULTIPLIER: f32 = 2.0;
const ANALOG_REFERENCE_HZ: f32 = 60.0;
const CINEMATIC_SECONDS_PER_E_FOLD: f32 = 0.06;

/// Vanilla's derived game sensitivity, which sets the mouse look curve.
///
/// It is recomputed only when the sensitivity option changes, so an option still at its default
/// leaves the separate game-sensitivity default in place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameSensitivity {
    sensitivity: f32,
    value: f32,
}

impl Default for GameSensitivity {
    fn default() -> Self {
        Self {
            sensitivity: DEFAULT_MOUSE_SENSITIVITY,
            value: DEFAULT_GAME_SENSITIVITY,
        }
    }
}

impl GameSensitivity {
    /// Applies a write of the `0.0..=1.0` sensitivity option, ignoring non-finite values.
    pub fn set_sensitivity(&mut self, sensitivity: f32) {
        let Some(sensitivity) = written_unit_option(self.sensitivity, sensitivity) else {
            return;
        };
        self.sensitivity = sensitivity;
        // Rounded once from double so every platform derives the same value.
        let curve = f64::from(sensitivity * 1.1).powf(f64::from(0.6125_f32)) as f32;
        if let Some(value) = written_unit_option(self.value, curve * 0.81) {
            self.value = value;
        }
    }

    #[must_use]
    pub const fn value(&self) -> f32 {
        self.value
    }
}

/// The value a vanilla unit-range float option stores for a write, or `None` if it keeps its own.
fn written_unit_option(current: f32, requested: f32) -> Option<f32> {
    ((current - requested).abs() > OPTION_EPSILON).then(|| requested.clamp(0.0, 1.0))
}

/// Degrees one frame's mouse counts turn the view, in vanilla's f32 order.
///
/// Vanilla divides counts by the window's pixel width, so a wider window turns less per count.
#[must_use]
pub fn mouse_turn_degrees(counts: Vec2, window_width: u32, game_sensitivity: f32) -> Vec2 {
    if window_width == 0 {
        return Vec2::ZERO;
    }
    let curve = game_sensitivity * 0.6 + 0.15;
    let curve = curve * curve * curve * 9600.0;
    counts / window_width as f32 * curve * 0.3
}

/// Degrees per analog look unit for a sensitivity slider fraction in `0..=1`.
#[must_use]
fn analog_degrees_per_look_unit(slider: f32) -> f32 {
    let slider = if slider.is_finite() {
        slider.clamp(0.0, 1.0)
    } else {
        0.5
    };
    let curve = slider * 0.6 + 0.2;
    curve * curve * curve * 8.0 * ANALOG_DEGREES_PER_UNIT_AT_UNIT_CURVE
}

/// Factor turning the router's already-multiplied analog look delta into degrees.
///
/// The router scales raw analog input by the linear multiplier; dividing it back out
/// lets the slider curve, not the multiplier, set the final angle.
#[must_use]
pub fn analog_degrees_per_routed_unit(multiplier: f32) -> f32 {
    let multiplier = if multiplier.is_finite() && multiplier > 0.0 {
        multiplier
    } else {
        1.0
    };
    analog_degrees_per_look_unit(multiplier / SLIDER_FULL_MULTIPLIER) / multiplier
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

    /// Vanilla's per-count turn, in degrees at a 1920-pixel window.
    fn per_count(game_sensitivity: f32) -> f32 {
        mouse_turn_degrees(Vec2::new(1.0, 0.0), 1920, game_sensitivity).x
    }

    fn after_writes(writes: &[f32]) -> f32 {
        let mut game = GameSensitivity::default();
        for &write in writes {
            game.set_sensitivity(write);
        }
        game.value()
    }

    #[test]
    fn mouse_turn_matches_vanilla_per_count_degrees() {
        assert_eq!(per_count(after_writes(&[])), 0.219_294_97);
        assert_eq!(per_count(after_writes(&[0.0])), 0.005_062_501);
        assert_eq!(per_count(after_writes(&[0.25])), 0.076_231_21);
        assert_eq!(per_count(after_writes(&[0.75])), 0.295_676_68);
        assert_eq!(per_count(after_writes(&[1.0])), 0.441_549_42);
        assert_eq!(per_count(after_writes(&[0.75, 0.5])), 0.173_234_21);
        let turn = mouse_turn_degrees(Vec2::new(-12.0, 5.0), 2560, after_writes(&[]));
        assert_eq!(turn, Vec2::new(-1.973_654_7, 0.822_356_1));
        assert_eq!(mouse_turn_degrees(Vec2::ONE, 0, 0.628), Vec2::ZERO);
    }

    /// The default slider once turned exact 0.15-degree steps, which servers flag as rounded aim.
    #[test]
    fn default_mouse_turn_is_not_a_round_step() {
        let game = after_writes(&[DEFAULT_MOUSE_SENSITIVITY]);
        assert_eq!(game, DEFAULT_GAME_SENSITIVITY);
        for width in [1280, 1920, 2560, 3024] {
            for counts in 1..=40 {
                let degrees = mouse_turn_degrees(Vec2::new(counts as f32, 0.0), width, game).x;
                let tenths = degrees * 10.0;
                assert!(
                    (tenths - tenths.round()).abs() > 1e-3,
                    "{counts} counts at {width}px turn {degrees} degrees"
                );
            }
        }
    }

    #[test]
    fn game_sensitivity_follows_only_real_option_changes() {
        assert_eq!(after_writes(&[0.5, 0.5005]), DEFAULT_GAME_SENSITIVITY);
        assert_eq!(after_writes(&[0.6]), DEFAULT_GAME_SENSITIVITY);
        assert_eq!(after_writes(&[0.7, 0.5]), after_writes(&[0.75, 0.5]));
        assert_eq!(after_writes(&[f32::NAN]), DEFAULT_GAME_SENSITIVITY);
        assert_eq!(after_writes(&[4.0]), after_writes(&[1.0]));
        assert!(after_writes(&[0.25]) < after_writes(&[0.75]));
    }

    #[test]
    fn analog_curve_is_monotonic_and_cancels_the_router_multiplier() {
        assert!(analog_degrees_per_look_unit(0.25) < analog_degrees_per_look_unit(0.75));
        assert_eq!(
            analog_degrees_per_look_unit(2.0),
            analog_degrees_per_look_unit(1.0)
        );
        assert_eq!(
            analog_degrees_per_look_unit(f32::NAN),
            analog_degrees_per_look_unit(0.5)
        );
        assert!((analog_degrees_per_routed_unit(1.0) - 0.15).abs() < 1e-6);
        assert!(analog_degrees_per_routed_unit(f32::NAN).is_finite());
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
