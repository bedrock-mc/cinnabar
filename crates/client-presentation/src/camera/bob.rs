//! Walk view-bob, expressed as view-space effects.

use std::f32::consts::PI;

use bevy::prelude::{Mat4, Resource, Vec3};

const WALK_DISTANCE_PER_BLOCK: f32 = 0.6;
const BOB_TARGET_CAP_PER_TICK: f32 = 0.1;
const BOB_KEEP_PER_TICK: f32 = 0.6;
const TELEPORT_BLOCKS: f32 = 8.0;
const TRANSLATION_GAIN_X: f32 = 0.65;
const ROLL_DEGREES: f32 = 3.0;
const PITCH_DEGREES: f32 = 5.0;
const PITCH_PHASE: f32 = 0.2;

/// A view-space transform applied to the world (and to the hand pass), not to the camera.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ViewEffect {
    pub translation: Vec3,
    pub roll_radians: f32,
    pub pitch_radians: f32,
}

impl ViewEffect {
    pub const NONE: Self = Self {
        translation: Vec3::ZERO,
        roll_radians: 0.0,
        pitch_radians: 0.0,
    };

    /// View matrix: translate, then roll, then pitch.
    #[must_use]
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_translation(self.translation)
            * Mat4::from_rotation_z(self.roll_radians)
            * Mat4::from_rotation_x(self.pitch_radians)
    }
}

/// Walk-cycle view effect from accumulated walk distance and smoothed bob amplitude.
#[must_use]
pub fn walk_bob_effect(walk_distance: f32, bob: f32) -> ViewEffect {
    if !walk_distance.is_finite() || !bob.is_finite() {
        return ViewEffect::NONE;
    }
    // The cycle runs backwards: the phase is the negated walk distance.
    let phase = (-(walk_distance as f64) * f64::from(PI)).rem_euclid(f64::from(2.0 * PI)) as f32;
    ViewEffect {
        translation: Vec3::new(
            phase.sin() * bob * TRANSLATION_GAIN_X,
            -(phase.cos() * bob).abs(),
            0.0,
        ),
        roll_radians: (phase.sin() * bob * ROLL_DEGREES).to_radians(),
        pitch_radians: ((phase - PITCH_PHASE).cos() * bob).abs().to_radians() * PITCH_DEGREES,
    }
}

/// Accumulates walk distance and the smoothed bob amplitude from per-frame positions.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct WalkBobState {
    walk_distance: f32,
    bob: f32,
    last_position: Option<Vec3>,
}

impl WalkBobState {
    #[must_use]
    pub const fn walk_distance(&self) -> f32 {
        self.walk_distance
    }

    #[must_use]
    pub const fn bob(&self) -> f32 {
        self.bob
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Advances by one rendered frame; a jump beyond the teleport bound restarts the baseline.
    pub fn advance(&mut self, position: Vec3, on_ground: bool, alive: bool, delta_seconds: f32) {
        if !position.is_finite() {
            return;
        }
        let horizontal = self
            .last_position
            .map_or(0.0, |last| (position.x - last.x).hypot(position.z - last.z));
        self.last_position = Some(position);
        if horizontal > TELEPORT_BLOCKS {
            self.bob = 0.0;
            return;
        }
        self.walk_distance += horizontal * WALK_DISTANCE_PER_BLOCK;
        if !(delta_seconds.is_finite() && delta_seconds > 0.0) {
            return;
        }
        let ticks = delta_seconds * simulation_clock::TICKS_PER_SECOND as f32;
        let per_tick_speed = horizontal / ticks;
        let target = if on_ground && alive {
            per_tick_speed.min(BOB_TARGET_CAP_PER_TICK)
        } else {
            0.0
        };
        let keep = BOB_KEEP_PER_TICK.powf(ticks.min(1000.0));
        self.bob += (target - self.bob) * (1.0 - keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_amplitude_is_identity() {
        assert_eq!(walk_bob_effect(3.7, 0.0), ViewEffect::NONE);
        assert!(walk_bob_effect(f32::NAN, 1.0) == ViewEffect::NONE);
    }

    #[test]
    fn bob_lifts_and_never_rises_above_rest() {
        for step in 0..40 {
            let effect = walk_bob_effect(step as f32 * 0.1, 0.1);
            assert!(effect.translation.y <= 0.0);
            assert!(effect.pitch_radians >= 0.0);
        }
    }

    #[test]
    fn walking_accumulates_distance_and_bob_only_on_ground() {
        let mut state = WalkBobState::default();
        let mut x = 0.0;
        for _ in 0..120 {
            x += 0.2158;
            state.advance(Vec3::new(x, 64.0, 0.0), true, true, 0.05);
        }
        assert!(state.walk_distance() > 1.0);
        assert!(state.bob() > 0.0 && state.bob() <= 0.1);
        for _ in 0..200 {
            x += 0.05;
            state.advance(Vec3::new(x, 64.0, 0.0), false, true, 0.05);
        }
        assert!(state.bob() < 1e-3);
    }

    #[test]
    fn teleport_restarts_without_a_bob_spike() {
        let mut state = WalkBobState::default();
        state.advance(Vec3::ZERO, true, true, 0.05);
        state.advance(Vec3::new(500.0, 0.0, 0.0), true, true, 0.05);
        assert_eq!(state.walk_distance(), 0.0);
        assert_eq!(state.bob(), 0.0);
    }
}
