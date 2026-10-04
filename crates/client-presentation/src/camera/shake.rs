//! Server-driven camera shake (positional and rotational); presentation-only.
//! Amplitude scales, frequencies and fade shape need native measurement.

use bevy::prelude::{EulerRot, Quat, Vec3};

const MAX_INTENSITY: f32 = 4.0;
const MAX_DURATION_SECONDS: f32 = 3600.0;
const POSITION_BLOCKS_PER_INTENSITY: f32 = 0.02;
const ROTATION_DEGREES_PER_INTENSITY: f32 = 0.5;
const FREQUENCIES_HZ: [f32; 3] = [17.0, 23.0, 29.0];

#[derive(Debug, Clone, Copy, PartialEq)]
struct ActiveShake {
    intensity: f32,
    duration: f32,
    elapsed: f32,
}

impl ActiveShake {
    fn new(intensity: f32, duration_seconds: f32) -> Option<Self> {
        if !(intensity.is_finite() && duration_seconds.is_finite()) || duration_seconds <= 0.0 {
            return None;
        }
        Some(Self {
            intensity: intensity.clamp(0.0, MAX_INTENSITY),
            duration: duration_seconds.min(MAX_DURATION_SECONDS),
            elapsed: 0.0,
        })
    }

    fn amplitude(&self) -> f32 {
        self.intensity * (1.0 - self.elapsed / self.duration).clamp(0.0, 1.0)
    }

    fn sample(&self) -> Vec3 {
        let t = self.elapsed;
        let axis = |phase: f32| {
            FREQUENCIES_HZ
                .iter()
                .enumerate()
                .map(|(index, hz)| {
                    ((t * hz + phase + index as f32 * 1.7) * std::f32::consts::TAU).sin()
                })
                .sum::<f32>()
                / FREQUENCIES_HZ.len() as f32
        };
        Vec3::new(axis(0.0), axis(0.31), axis(0.67)) * self.amplitude()
    }
}

/// Which shake kinds a server command targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShakeKind {
    Positional,
    Rotational,
}

/// Camera-local offset produced by the active shakes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShakeOffset {
    pub translation: Vec3,
    pub rotation: Quat,
}

impl ShakeOffset {
    pub const NONE: Self = Self {
        translation: Vec3::ZERO,
        rotation: Quat::IDENTITY,
    };
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct ShakeState {
    positional: Option<ActiveShake>,
    rotational: Option<ActiveShake>,
}

impl ShakeState {
    /// Starts a shake of `kind`, replacing one in flight; returns whether it was accepted.
    pub fn add(&mut self, kind: ShakeKind, intensity: f32, duration_seconds: f32) -> bool {
        let shake = ActiveShake::new(intensity, duration_seconds);
        match kind {
            ShakeKind::Positional => self.positional = shake,
            ShakeKind::Rotational => self.rotational = shake,
        }
        shake.is_some()
    }

    pub fn stop_all(&mut self) {
        *self = Self::default();
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.positional.is_some() || self.rotational.is_some()
    }

    pub fn advance(&mut self, delta_seconds: f32) {
        if !(delta_seconds.is_finite() && delta_seconds > 0.0) {
            return;
        }
        for slot in [&mut self.positional, &mut self.rotational] {
            if let Some(shake) = slot.as_mut() {
                shake.elapsed += delta_seconds;
                if shake.elapsed >= shake.duration {
                    *slot = None;
                }
            }
        }
    }

    #[must_use]
    pub fn offset(&self) -> ShakeOffset {
        let translation = self.positional.map_or(Vec3::ZERO, |shake| {
            shake.sample() * POSITION_BLOCKS_PER_INTENSITY
        });
        let rotation = self.rotational.map_or(Quat::IDENTITY, |shake| {
            let degrees = shake.sample() * ROTATION_DEGREES_PER_INTENSITY;
            Quat::from_euler(
                EulerRot::XYZ,
                degrees.x.to_radians(),
                degrees.y.to_radians(),
                degrees.z.to_radians(),
            )
        });
        ShakeOffset {
            translation,
            rotation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expires_after_its_duration_and_fades_out() {
        let mut state = ShakeState::default();
        assert!(state.add(ShakeKind::Positional, 2.0, 1.0));
        let peak = |state: &mut ShakeState, steps: usize| {
            let mut peak = 0.0_f32;
            for _ in 0..steps {
                state.advance(0.01);
                peak = peak.max(state.offset().translation.length());
            }
            peak
        };
        let early = peak(&mut state, 30);
        peak(&mut state, 40);
        let late = peak(&mut state, 25);
        assert!(early > 0.0 && late < early);
        state.advance(0.1);
        assert!(!state.is_active());
        assert_eq!(state.offset(), ShakeOffset::NONE);
    }

    #[test]
    fn kinds_are_independent_and_stop_clears_both() {
        let mut state = ShakeState::default();
        state.add(ShakeKind::Rotational, 1.0, 5.0);
        state.advance(0.1);
        assert_eq!(state.offset().translation, Vec3::ZERO);
        assert_ne!(state.offset().rotation, Quat::IDENTITY);
        state.add(ShakeKind::Positional, 1.0, 5.0);
        state.stop_all();
        assert!(!state.is_active());
    }

    #[test]
    fn malformed_shakes_are_rejected() {
        let mut state = ShakeState::default();
        assert!(!state.add(ShakeKind::Positional, f32::NAN, 1.0));
        assert!(!state.add(ShakeKind::Positional, 1.0, 0.0));
        assert!(!state.is_active());
    }

    #[test]
    fn intensity_is_clamped() {
        let mut state = ShakeState::default();
        state.add(ShakeKind::Positional, 1000.0, 10.0);
        state.advance(0.01);
        assert!(
            state.offset().translation.length()
                <= MAX_INTENSITY * POSITION_BLOCKS_PER_INTENSITY * 2.0
        );
    }
}
