//! Walk view-bob and first-person hand sway, expressed as view-space effects, following
//! vanilla's view bob and hand spring.

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
        let ticks = delta_seconds * world::TICKS_PER_SECOND as f32;
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

/// First-person hand sway: a damped spring driven by the smoothed view turn rate, per the 26.30
/// reference (turn rates in Minecraft degrees, spring offsets in degrees).
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct HandSwayState {
    last: Option<(f32, f32)>,
    rate: (f32, f32),
    offset: (f32, f32),
    velocity: (f32, f32),
}

const SWAY_RATE_KEEP: f32 = 0.8;
const SWAY_RATE_LIMIT: f32 = 50.0;
const SWAY_STIFFNESS: f32 = 900.0;
const SWAY_DAMPING: f32 = 42.0;
const SWAY_DRIVE: f32 = 90.0;
const SWAY_MAX_STEP: f32 = 1.0 / 120.0;
const SWAY_MAX_DELTA: f32 = 0.2;
const SWAY_VELOCITY_RESET: f32 = 1000.0;

impl HandSwayState {
    /// Extra `(pitch, yaw)` hand rotation in radians, about the view X then Y axes.
    #[must_use]
    pub fn sway_radians(&self) -> (f32, f32) {
        (self.offset.0.to_radians(), self.offset.1.to_radians())
    }

    /// Advances by one frame from the view's pitch (up positive) and yaw in radians.
    pub fn advance(&mut self, pitch: f32, yaw: f32, delta_seconds: f32) {
        if !(pitch.is_finite() && yaw.is_finite()) {
            return;
        }
        // Minecraft pitch grows looking down and yaw grows turning right.
        let view = (-pitch.to_degrees(), -yaw.to_degrees());
        let (last_pitch, last_yaw) = self.last.replace(view).unwrap_or(view);
        let dt = if delta_seconds.is_finite() && delta_seconds >= 0.0 {
            delta_seconds.min(SWAY_MAX_DELTA)
        } else {
            SWAY_MAX_DELTA
        };
        if dt == 0.0 {
            return;
        }
        let turn = (
            (view.0 - last_pitch) / dt,
            shortest_degrees(view.1 - last_yaw) / dt,
        );
        let smooth = |old: f32, rate: f32| {
            (old * SWAY_RATE_KEEP + rate * (1.0 - SWAY_RATE_KEEP))
                .clamp(-SWAY_RATE_LIMIT, SWAY_RATE_LIMIT)
        };
        self.rate = (smooth(self.rate.0, turn.0), smooth(self.rate.1, turn.1));
        let steps = (dt / SWAY_MAX_STEP).ceil().max(1.0);
        let h = dt / steps;
        for _ in 0..steps as u32 {
            for (x, v, rate) in [
                (&mut self.offset.0, &mut self.velocity.0, self.rate.0),
                (&mut self.offset.1, &mut self.velocity.1, self.rate.1),
            ] {
                *v += (-SWAY_STIFFNESS * *x - SWAY_DAMPING * *v + SWAY_DRIVE * rate) * h;
                *x += h * *v;
                if v.abs() >= SWAY_VELOCITY_RESET {
                    *v = 0.0;
                }
            }
        }
    }
}

pub(super) fn shortest_degrees(delta: f32) -> f32 {
    (delta + 180.0).rem_euclid(360.0) - 180.0
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

    // Looking up swings the hand down (it trails the view), then the spring settles.
    #[test]
    fn sway_trails_rotation_and_decays_when_still() {
        let mut sway = HandSwayState::default();
        sway.advance(0.0, 0.0, 0.05);
        sway.advance(0.5, 0.0, 0.05);
        assert!(sway.sway_radians().0 < 0.0);
        for _ in 0..200 {
            sway.advance(0.5, 0.0, 0.05);
        }
        assert!(sway.sway_radians().0.abs() < 1e-4);
    }

    // A sustained turn settles at a tenth of the (capped) turn rate: at most five degrees.
    #[test]
    fn sustained_turn_settles_at_the_capped_offset() {
        let mut sway = HandSwayState::default();
        for frame in 0..400 {
            sway.advance(0.0, -(frame as f32) * 0.1, 0.01);
        }
        assert!((sway.sway_radians().1 - 5.0_f32.to_radians()).abs() < 1e-3);
    }

    #[test]
    fn sway_takes_the_short_way_around_the_yaw_seam() {
        let (mut seam, mut plain) = (HandSwayState::default(), HandSwayState::default());
        seam.advance(0.0, PI - 0.01, 0.05);
        seam.advance(0.0, -PI + 0.01, 0.05);
        plain.advance(0.0, 0.0, 0.05);
        plain.advance(0.0, 0.02, 0.05);
        assert!((seam.sway_radians().1 - plain.sway_radians().1).abs() < 1e-4);
    }
}
