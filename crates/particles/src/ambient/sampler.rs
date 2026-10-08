//! Vanilla time-limited ambient sampler.

use std::time::Duration;

use super::random::AmbientRandom;

pub const MIN_SAMPLES: u32 = 100;
pub(super) const NEAR_SAMPLES: u32 = 667;
pub(super) const MAX_SAMPLES: u32 = NEAR_SAMPLES * 2;
const NEAR_RADIUS: u32 = 16;
const FAR_RADIUS: u32 = 32;
const MAX_MOVING_RADIUS: u32 = 24;
const SAMPLE_BUDGET_MILLISECONDS: f32 = 0.25;
const FULL_RATE_THRESHOLD: f32 = 0.95;
const TELEPORT_DISTANCE: f32 = 88.0;
const MOVEMENT_SCALE: f32 = 4.640_371_3;
const FORWARD_BIAS: f32 = 0.2155;
const CENTER_SHIFT_SCALE: f32 = 8.0;
const RADIUS_GROWTH_SCALE: f32 = 4.0;
const NORMALIZE_EPSILON: f32 = 0.0001;

pub struct Sampler {
    pub sample_count: u32,
    previous_position: [f32; 3],
}

impl Default for Sampler {
    fn default() -> Self {
        // The target version starts mode 2 at 100 samples with its previous camera position zeroed.
        Self {
            sample_count: MIN_SAMPLES,
            previous_position: [0.0; 3],
        }
    }
}

pub struct SamplePlan {
    pub(crate) center: [i32; 3],
    pub(crate) near_radius: u32,
    pub sample_count: u32,
}

impl Sampler {
    pub fn plan(&self, position: [f32; 3], forward: [f32; 3]) -> SamplePlan {
        let mut center = position.map(|component| component.floor() as i32);
        let mut near_radius = NEAR_RADIUS;
        let delta: [f32; 3] =
            std::array::from_fn(|axis| position[axis] - self.previous_position[axis]);
        let speed = delta
            .iter()
            .map(|component| component * component)
            .sum::<f32>()
            .sqrt();
        if self.sample_count as f32 / (MAX_SAMPLES as f32) < FULL_RATE_THRESHOLD
            && speed < TELEPORT_DISTANCE
        {
            let movement = speed * MOVEMENT_SCALE;
            if delta.iter().zip(forward).map(|(a, b)| a * b).sum::<f32>() > 0.0 {
                let shift = (movement * CENTER_SHIFT_SCALE).clamp(0.0, NEAR_RADIUS as f32);
                let adjusted: [f32; 3] =
                    std::array::from_fn(|axis| delta[axis] + forward[axis] * FORWARD_BIAS);
                let length = adjusted
                    .iter()
                    .map(|component| component * component)
                    .sum::<f32>()
                    .sqrt();
                if length >= NORMALIZE_EPSILON {
                    center = std::array::from_fn(|axis| {
                        (position[axis] + adjusted[axis] / length * shift).floor() as i32
                    });
                }
            }
            near_radius = (NEAR_RADIUS + (movement * RADIUS_GROWTH_SCALE) as u32)
                .clamp(NEAR_RADIUS, MAX_MOVING_RADIUS);
        }
        SamplePlan {
            center,
            near_radius,
            sample_count: self.sample_count,
        }
    }

    pub fn finish(&mut self, position: [f32; 3], elapsed: Duration) {
        let elapsed_ms = elapsed.as_secs_f32() * 1000.0;
        self.sample_count = if elapsed_ms > 0.0 {
            (SAMPLE_BUDGET_MILLISECONDS / (elapsed_ms / self.sample_count as f32)).ceil() as u32
        } else {
            MIN_SAMPLES
        }
        .clamp(MIN_SAMPLES, MAX_SAMPLES);
        self.previous_position = position;
    }
}

impl SamplePlan {
    pub fn sample(&self, index: u32, random: &mut AmbientRandom) -> Option<[i32; 3]> {
        let radius = if index < NEAR_SAMPLES {
            self.near_radius
        } else {
            FAR_RADIUS
        };
        let z = random.gaussian_int(radius);
        let y = random.gaussian_int(radius);
        let x = random.gaussian_int(radius);
        Some([
            self.center[0].checked_add(x)?,
            self.center[1].checked_add(y)?,
            self.center[2].checked_add(z)?,
        ])
    }
}
