//! Dynamic FOV: movement speed, slowness, flying, bow draw and spyglass scaling with per-tick smoothing.

use bevy::prelude::Resource;

/// Spyglass zoom target for the FOV multiplier.
pub const SPYGLASS_FOV_MODIFIER: f32 = 0.1;
/// Vanilla's default walk-speed ability, used until an ability layer sets one.
pub const DEFAULT_WALK_SPEED_ABILITY: f32 = 0.1;
const SPEED_RATIO_SCALE: f32 = 1.2;
const SLOWNESS_STEP: f32 = -0.1;
const SLOWNESS_FLOOR: f32 = 0.01;
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
    /// Effective movement-speed attribute, including the local sprint modifier.
    pub movement_speed: f32,
    /// Resolved walk-speed ability.
    pub walk_speed: f32,
    pub flying: bool,
    /// Zero-based amplifier of an active slowness effect.
    pub slowness_amplifier: Option<i32>,
    pub bow_draw_seconds: Option<f32>,
    pub spyglass_scoping: bool,
    /// "FOV effects" scale: 0 disables speed-driven changes, 1 is full strength.
    pub fov_effects_scale: f32,
}

impl Default for CameraFovInputs {
    fn default() -> Self {
        Self {
            movement_speed: sim::DEFAULT_MOVEMENT_SPEED as f32,
            walk_speed: DEFAULT_WALK_SPEED_ABILITY,
            flying: false,
            slowness_amplifier: None,
            bow_draw_seconds: None,
            spyglass_scoping: false,
            fov_effects_scale: 1.0,
        }
    }
}

impl CameraFovInputs {
    /// Unsmoothed FOV multiplier; spyglass zoom ignores the FOV-effects scale.
    ///
    /// Slowness replaces the movement-speed term rather than scaling it.
    #[must_use]
    pub fn target_modifier(&self) -> f32 {
        if self.spyglass_scoping {
            return SPYGLASS_FOV_MODIFIER;
        }
        let mut modifier = if self.flying { FLYING_MODIFIER } else { 1.0 };
        modifier = match self.slowness_amplifier {
            None => {
                modifier * ((self.movement_speed / self.walk_speed) * SPEED_RATIO_SCALE + 1.0) * 0.5
            }
            Some(amplifier) => {
                modifier * (amplifier as f32 * SLOWNESS_STEP + 1.0).max(SLOWNESS_FLOOR)
            }
        };
        if let Some(seconds) = self.bow_draw_seconds.filter(|seconds| seconds.is_finite()) {
            let draw = (seconds / BOW_FULL_DRAW_SECONDS).clamp(0.0, 1.0);
            modifier *= 1.0 - BOW_MAX_ZOOM * draw * draw;
        }
        if !modifier.is_finite() {
            return 1.0;
        }
        let scale = if self.fov_effects_scale.is_finite() {
            self.fov_effects_scale.clamp(0.0, 1.0)
        } else {
            1.0
        };
        modifier = 1.0 + (modifier - 1.0) * scale;
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

    const SPRINTING_SPEED: f32 = 0.1 * 1.3;

    /// Vanilla widens the base view by its speed ratio: 1.1 walking, 1.28 sprinting.
    #[test]
    fn walking_and_sprinting_follow_the_speed_ratio() {
        let mut inputs = CameraFovInputs::default();
        assert!((inputs.target_modifier() - 1.1).abs() < 1e-6);
        inputs.movement_speed = SPRINTING_SPEED;
        assert!((inputs.target_modifier() - 1.28).abs() < 1e-6);
        inputs.flying = true;
        assert!((inputs.target_modifier() - 1.408).abs() < 1e-6);
        inputs.walk_speed = 0.2;
        assert!((inputs.target_modifier() - 1.1 * 0.89).abs() < 1e-6);
    }

    /// Slowness replaces the speed term, so a sprinting slowed player does not widen.
    #[test]
    fn slowness_replaces_the_speed_term() {
        let mut inputs = CameraFovInputs {
            movement_speed: SPRINTING_SPEED,
            slowness_amplifier: Some(0),
            ..Default::default()
        };
        assert!((inputs.target_modifier() - 1.0).abs() < 1e-6);
        inputs.slowness_amplifier = Some(1);
        assert!((inputs.target_modifier() - 0.9).abs() < 1e-6);
        inputs.slowness_amplifier = Some(20);
        assert!((inputs.target_modifier() - MIN_MODIFIER).abs() < 1e-6);
    }

    #[test]
    fn effects_scale_zero_disables_speed_fov_but_not_spyglass() {
        let inputs = CameraFovInputs {
            movement_speed: SPRINTING_SPEED,
            flying: true,
            bow_draw_seconds: Some(5.0),
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
    fn bow_zoom_saturates_on_the_walking_base() {
        let bow = CameraFovInputs {
            bow_draw_seconds: Some(5.0),
            ..Default::default()
        };
        assert!((bow.target_modifier() - 1.1 * 0.85).abs() < 1e-6);
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
        let zero_walk = CameraFovInputs {
            walk_speed: 0.0,
            ..Default::default()
        };
        assert_eq!(zero_walk.target_modifier(), 1.0);
        let mut state = CameraFovState::default();
        state.advance(f32::NAN, 0.05);
        assert!(state.modifier().is_finite());
        state.advance(0.5, f32::NAN);
        assert!(state.modifier().is_finite());
    }
}
