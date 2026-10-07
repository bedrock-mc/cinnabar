//! Queued positional and rotational camera shake with independent intensity envelopes.

mod noise;

use bevy::prelude::{EulerRot, Quat, Transform, Vec2, Vec3};

const MAX_INTENSITY: f32 = 4.0;
const MAX_QUEUED_SHAKES: usize = 4096;
const DECAY_PER_SECOND: f32 = 1.0;
const FREQUENCY: f32 = 10.0;
const AMPLITUDE_RADIANS: f32 = 5.0 * std::f32::consts::PI / 180.0;
const NOISE_MULTIPLIER: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct ShakeEvent {
    intensity: f32,
    remaining: f32,
}

#[derive(Debug, Default, Clone, PartialEq)]
struct ShakeQueue {
    events: Vec<ShakeEvent>,
    intensity: f32,
}

impl ShakeQueue {
    /// Active events add together; the previous peak decays as their combined floor falls.
    fn advance(&mut self, delta: f32) {
        let total = self
            .events
            .iter()
            .fold(0.0, |sum, event| sum + event.intensity);
        self.intensity = (self.intensity - DECAY_PER_SECOND * delta).max(total.min(MAX_INTENSITY));
        for event in &mut self.events {
            event.remaining -= delta;
        }
        self.events.retain(|event| event.remaining > 0.0);
    }
}

/// Which shake kinds a server command targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShakeKind {
    Positional,
    Rotational,
}

/// World-space translation and Euler pitch/yaw perturbations produced by active shakes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShakeOffset {
    pub translation: Vec3,
    pub rotation_radians: Option<Vec2>,
}

impl ShakeOffset {
    pub const NONE: Self = Self {
        translation: Vec3::ZERO,
        rotation_radians: None,
    };

    /// Adds positional noise in world axes and rotational noise in the camera's Euler axes.
    pub fn apply(self, pose: &mut Transform) {
        pose.translation += self.translation;
        if let Some(shake) = self.rotation_radians {
            let (yaw, pitch, _) = pose.rotation.to_euler(EulerRot::YXZ);
            pose.rotation = Quat::from_euler(EulerRot::YXZ, yaw - shake.y, pitch - shake.x, 0.0);
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ShakeState {
    positional: ShakeQueue,
    rotational: ShakeQueue,
    noise: Option<noise::Noise>,
    elapsed: f32,
}

impl ShakeState {
    /// Queues a positive finite shake without replacing earlier commands of the same kind.
    pub fn add(&mut self, kind: ShakeKind, intensity: f32, duration_seconds: f32) -> bool {
        if !intensity.is_finite()
            || !duration_seconds.is_finite()
            || intensity <= 0.0
            || duration_seconds <= 0.0
        {
            return false;
        }
        let queue = match kind {
            ShakeKind::Positional => &mut self.positional,
            ShakeKind::Rotational => &mut self.rotational,
        };
        if queue.events.len() == MAX_QUEUED_SHAKES {
            return false;
        }
        queue.events.push(ShakeEvent {
            intensity,
            remaining: duration_seconds,
        });
        self.noise.get_or_insert_with(noise::Noise::default);
        true
    }

    /// Removes both queue types immediately while retaining their allocated buffers.
    pub fn stop_all(&mut self) {
        for queue in [&mut self.positional, &mut self.rotational] {
            queue.events.clear();
            queue.intensity = 0.0;
        }
        self.elapsed = 0.0;
        self.noise = None;
    }

    /// Reports queued events independently of their current sampled intensity.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.positional.events.is_empty() || !self.rotational.events.is_empty()
    }

    /// Advances both envelopes without allocating; final expiry removes the whole effect.
    pub fn advance(&mut self, delta_seconds: f32) {
        if !(delta_seconds.is_finite() && delta_seconds > 0.0 && self.is_active()) {
            return;
        }
        self.positional.advance(delta_seconds);
        self.rotational.advance(delta_seconds);
        self.elapsed += delta_seconds;
        if !self.is_active() {
            self.stop_all();
        }
    }

    /// Samples the shared noise fields with independent positional and rotational intensities.
    #[must_use]
    pub fn offset(&self) -> ShakeOffset {
        let Some(noise) = &self.noise else {
            return ShakeOffset::NONE;
        };
        let noise = noise.sample(self.elapsed * NOISE_MULTIPLIER, FREQUENCY)
            * AMPLITUDE_RADIANS
            * NOISE_MULTIPLIER;
        ShakeOffset {
            translation: noise * self.positional.intensity,
            rotation_radians: (self.rotational.intensity > 0.0)
                .then_some(noise.truncate() * self.rotational.intensity),
        }
    }
}

#[cfg(test)]
mod tests;
