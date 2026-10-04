//! Dynamic FOV: sprint, speed/slowness, flying, bow draw and spyglass scaling with per-tick smoothing.
//! Magnitudes are provisional and need native measurement.

use bevy::prelude::Resource;

/// Spyglass zoom target for the FOV multiplier.
pub const SPYGLASS_FOV_MODIFIER: f32 = 0.1;
const SPRINT_SPEED_BONUS: f32 = 0.3;
const SPEED_BONUS_PER_LEVEL: f32 = 0.2;
const SLOWNESS_PENALTY_PER_LEVEL: f32 = 0.15;
const FLYING_MODIFIER: f32 = 1.1;
const BOW_FULL_DRAW_SECONDS: f32 = 1.0;
const BOW_MAX_ZOOM: f32 = 0.15;
const SMOOTHING_PER_TICK: f32 = 0.5;
const TICKS_PER_SECOND: f32 = 20.0;
const MIN_MODIFIER: f32 = 0.05;
const MAX_MODIFIER: f32 = 2.0;

/// Gameplay facts that steer the FOV multiplier; the equipment lane owns `bow_draw_seconds` and `spyglass_scoping`.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct CameraFovInputs {
    pub sprinting: bool,
    pub flying: bool,
    pub speed_levels: u32,
    pub slowness_levels: u32,
    pub bow_draw_seconds: Option<f32>,
    pub spyglass_scoping: bool,
    /// "FOV effects" scale: 0 disables speed-driven changes, 1 is full strength.
    pub fov_effects_scale: f32,
}

impl Default for CameraFovInputs {
    fn default() -> Self {
        Self {
            sprinting: false,
            flying: false,
            speed_levels: 0,
            slowness_levels: 0,
            bow_draw_seconds: None,
            spyglass_scoping: false,
            fov_effects_scale: 1.0,
        }
    }
}

impl CameraFovInputs {
    /// Unsmoothed FOV multiplier; spyglass zoom ignores the FOV-effects scale.
    #[must_use]
    pub fn target_modifier(&self) -> f32 {
        if self.spyglass_scoping {
            return SPYGLASS_FOV_MODIFIER;
        }
        let mut speed_ratio =
            1.0 + if self.sprinting {
                SPRINT_SPEED_BONUS
            } else {
                0.0
            } + SPEED_BONUS_PER_LEVEL * self.speed_levels.min(255) as f32
                - SLOWNESS_PENALTY_PER_LEVEL * self.slowness_levels.min(255) as f32;
        speed_ratio = speed_ratio.max(0.0);
        let mut modifier = (speed_ratio + 1.0) * 0.5;
        if self.flying {
            modifier *= FLYING_MODIFIER;
        }
        let scale = if self.fov_effects_scale.is_finite() {
            self.fov_effects_scale.clamp(0.0, 1.0)
        } else {
            1.0
        };
        modifier = 1.0 + (modifier - 1.0) * scale;
        if let Some(seconds) = self.bow_draw_seconds.filter(|seconds| seconds.is_finite()) {
            let draw = (seconds / BOW_FULL_DRAW_SECONDS).clamp(0.0, 1.0);
            modifier *= 1.0 - BOW_MAX_ZOOM * draw * draw;
        }
        modifier.clamp(MIN_MODIFIER, MAX_MODIFIER)
    }
}

/// Smoothed FOV multiplier, frame-rate independent against a per-tick blend.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct CameraFovState {
    modifier: f32,
}

impl Default for CameraFovState {
    fn default() -> Self {
        Self { modifier: 1.0 }
    }
}

impl CameraFovState {
    #[must_use]
    pub const fn modifier(&self) -> f32 {
        self.modifier
    }

    /// Blends toward `target` over `delta_seconds` and returns the new multiplier.
    pub fn advance(&mut self, target: f32, delta_seconds: f32) -> f32 {
        let target = if target.is_finite() { target } else { 1.0 };
        if delta_seconds.is_finite() && delta_seconds > 0.0 {
            let keep = SMOOTHING_PER_TICK.powf((delta_seconds * TICKS_PER_SECOND).min(1000.0));
            self.modifier += (target - self.modifier) * (1.0 - keep);
        }
        self.modifier = self.modifier.clamp(MIN_MODIFIER, MAX_MODIFIER);
        self.modifier
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_is_neutral_and_sprint_widens() {
        let mut inputs = CameraFovInputs::default();
        assert!((inputs.target_modifier() - 1.0).abs() < 1e-6);
        inputs.sprinting = true;
        assert!((inputs.target_modifier() - 1.15).abs() < 1e-6);
    }

    #[test]
    fn effects_scale_zero_disables_speed_fov_but_not_spyglass() {
        let inputs = CameraFovInputs {
            sprinting: true,
            flying: true,
            fov_effects_scale: 0.0,
            ..Default::default()
        };
        assert!((inputs.target_modifier() - 1.0).abs() < 1e-6);
        let scoped = CameraFovInputs {
            spyglass_scoping: true,
            fov_effects_scale: 0.0,
            ..Default::default()
        };
        assert!((scoped.target_modifier() - SPYGLASS_FOV_MODIFIER).abs() < 1e-6);
    }

    #[test]
    fn slowness_narrows_and_bow_zoom_saturates() {
        let slow = CameraFovInputs {
            slowness_levels: 1,
            ..Default::default()
        };
        assert!(slow.target_modifier() < 1.0);
        let bow = CameraFovInputs {
            bow_draw_seconds: Some(5.0),
            ..Default::default()
        };
        assert!((bow.target_modifier() - 0.85).abs() < 1e-6);
    }

    #[test]
    fn smoothing_halves_the_gap_per_tick_regardless_of_frame_rate() {
        let mut one = CameraFovState::default();
        one.advance(0.1, 0.05);
        let mut many = CameraFovState::default();
        for _ in 0..5 {
            many.advance(0.1, 0.01);
        }
        assert!((one.modifier() - 0.55).abs() < 1e-5);
        assert!((one.modifier() - many.modifier()).abs() < 1e-5);
    }

    #[test]
    fn malformed_values_are_ignored() {
        let mut state = CameraFovState::default();
        state.advance(f32::NAN, 0.05);
        assert!(state.modifier().is_finite());
        state.advance(0.5, f32::NAN);
        assert!(state.modifier().is_finite());
    }
}
