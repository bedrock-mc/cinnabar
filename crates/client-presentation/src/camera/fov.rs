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
const MIN_MODIFIER: f32 = 0.05;
const MAX_MODIFIER: f32 = 2.0;
const DEATH_CAMERA_BASE_FOV: f32 = 60.0;
const DEATH_CAMERA_FOV_INCREASE: f32 = 0.4;
const DEATH_CAMERA_FOV_TICKS: f32 = 120.0;
const PLAYER_DEATH_FOV_TICKS: f32 = 500.0;

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
    /// Signed actor death counter and the current actor tick fraction, absent while alive.
    pub death_ticks: Option<f32>,
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
            death_ticks: None,
        }
    }
}

impl CameraFovInputs {
    /// Samples actor-owned death time after its tick advance and clears it on recovery.
    pub fn set_death_ticks(&mut self, ticks: Option<i16>, partial_tick: f32) {
        let partial_tick = if partial_tick.is_finite() {
            partial_tick.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.death_ticks = ticks.map(|ticks| f32::from(ticks) + partial_tick);
    }

    /// Resolves player-state FOV for the selected camera without smoothing the death curve.
    #[must_use]
    pub fn death_fov_degrees(&self, current_fov: f32, death_camera: bool) -> f32 {
        let Some(ticks) = self.death_ticks.filter(|ticks| ticks.is_finite()) else {
            return current_fov;
        };
        if death_camera {
            let progress = (ticks / DEATH_CAMERA_FOV_TICKS).clamp(0.0, 1.0);
            DEATH_CAMERA_BASE_FOV
                * (1.0 + DEATH_CAMERA_FOV_INCREASE * (progress * std::f32::consts::FRAC_PI_2).sin())
        } else {
            current_fov
                / (1.0 + 2.0 * (1.0 - PLAYER_DEATH_FOV_TICKS / (ticks + PLAYER_DEATH_FOV_TICKS)))
        }
    }

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
            let keep = SMOOTHING_PER_TICK
                .powf((delta_seconds * world::TICKS_PER_SECOND as f32).min(1000.0));
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

    #[test]
    fn modern_death_fov_follows_actor_ticks_beyond_body_animation_and_clears_on_recovery() {
        let mut inputs = CameraFovInputs::default();
        inputs.set_death_ticks(Some(0), 0.0);
        assert_eq!(inputs.death_fov_degrees(110.0, true), 60.0);
        inputs.set_death_ticks(Some(59), 1.0);
        assert!((inputs.death_fov_degrees(30.0, true) - 76.97056).abs() < 0.0001);
        for ticks in [120, 125, 1000, i16::MAX] {
            inputs.set_death_ticks(Some(ticks), 0.0);
            assert_eq!(inputs.death_fov_degrees(70.0, true), 84.0);
        }
        inputs.set_death_ticks(Some(i16::MIN), 0.0);
        assert_eq!(inputs.death_fov_degrees(70.0, true), 60.0);
        inputs.set_death_ticks(None, 0.5);
        assert_eq!(inputs.death_fov_degrees(110.0, true), 110.0);
    }

    #[test]
    fn death_fov_without_the_death_camera_uses_the_current_fov() {
        let mut inputs = CameraFovInputs::default();
        inputs.set_death_ticks(Some(0), 0.0);
        assert_eq!(inputs.death_fov_degrees(110.0, false), 110.0);
        inputs.set_death_ticks(Some(500), 0.0);
        assert_eq!(inputs.death_fov_degrees(110.0, false), 55.0);
    }

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

    /// Attribute resends and sprint restarts retain one speed contribution to FOV.
    #[test]
    fn sprint_fov_does_not_overshoot_after_effective_attribute_resends() {
        use client_world::MovementSpeedAttribute;
        use protocol::{ActorAttribute, ActorAttributeModifier};
        use std::sync::Arc;

        let factor = sim::SPRINT_SPEED_MULTIPLIER as f32;
        let mut packet = ActorAttribute {
            name: Arc::from("minecraft:movement"),
            min: 0.0,
            max: f32::MAX,
            current: SPRINTING_SPEED,
            default: Some(sim::DEFAULT_MOVEMENT_SPEED as f32),
            modifiers: Arc::from([]),
        };
        let mut inputs = CameraFovInputs::default();
        for modifiers in [
            Arc::from([]),
            Arc::from([ActorAttributeModifier {
                id: Arc::from(client_world::SPRINT_SPEED_MODIFIER_ID),
                name: Arc::from("sprint"),
                amount: factor - 1.0,
                operation: 2,
                operand: 2,
                serializable: false,
            }]),
        ] {
            packet.modifiers = modifiers;
            let mut speed = MovementSpeedAttribute::from_attribute(&packet).unwrap();
            for _ in 0..5 {
                inputs.movement_speed = speed.current as f32;
                assert!((inputs.target_modifier() - 1.28).abs() < 1e-6);
                speed.set_sprint_modifier(None);
                inputs.movement_speed = speed.current as f32;
                let stopped = if packet.modifiers.is_empty() {
                    1.28
                } else {
                    1.1
                };
                assert!((inputs.target_modifier() - stopped).abs() < 1e-6);
                speed.set_sprint_modifier(Some(factor));
                inputs.movement_speed = speed.current as f32;
                assert!((inputs.target_modifier() - 1.28).abs() < 1e-6);
                // A server resend replaces the local modifier set and effective current.
                speed = MovementSpeedAttribute::from_attribute(&packet).unwrap();
            }
        }
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
        one.advance(0.1, world::TICK_DURATION.as_secs_f32());
        let mut many = CameraFovState::default();
        for _ in 0..5 {
            many.advance(0.1, world::TICK_DURATION.as_secs_f32() / 5.0);
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
