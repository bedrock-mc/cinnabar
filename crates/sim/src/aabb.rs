use serde::{Deserialize, Serialize};

use crate::Vec3;

pub const PLAYER_WIDTH: f64 = 0.6;
pub const PLAYER_HEIGHT: f64 = 1.8;
/// bedsim shrinks each horizontal half-extent by this amount.
pub const PLAYER_HORIZONTAL_EPSILON: f64 = 1.0e-4;

/// Axis-aligned collision box with inclusive contact faces.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    #[must_use]
    pub const fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    #[must_use]
    pub fn player_at(feet: Vec3) -> Self {
        let half_width = PLAYER_WIDTH * 0.5 - PLAYER_HORIZONTAL_EPSILON;
        Self::new(
            Vec3::new(feet.x - half_width, feet.y, feet.z - half_width),
            Vec3::new(
                feet.x + half_width,
                feet.y + PLAYER_HEIGHT,
                feet.z + half_width,
            ),
        )
    }

    #[must_use]
    pub fn translated(self, delta: Vec3) -> Self {
        Self::new(self.min + delta, self.max + delta)
    }

    #[must_use]
    pub fn swept(self, delta: Vec3) -> Self {
        let end = self.translated(delta);
        Self::new(
            self.min.component_min(end.min),
            self.max.component_max(end.max),
        )
    }

    #[must_use]
    pub fn grown(self, amount: f64) -> Self {
        let amount = Vec3::new(amount, amount, amount);
        Self::new(self.min - amount, self.max + amount)
    }

    #[must_use]
    pub fn intersects(self, rhs: Self) -> bool {
        self.max.x > rhs.min.x
            && self.min.x < rhs.max.x
            && self.max.y > rhs.min.y
            && self.min.y < rhs.max.y
            && self.max.z > rhs.min.z
            && self.min.z < rhs.max.z
    }

    #[must_use]
    pub fn is_zero_volume(self) -> bool {
        self.min == self.max
    }

    /// Clips this moving box's velocity against one stationary box using the
    /// bedsim/Oomph swept-AABB algorithm.
    #[must_use]
    pub fn clip_against(self, stationary: Self, velocity: Vec3) -> Vec3 {
        if stationary.is_zero_volume() {
            return velocity;
        }

        let mut axis_penetrations = [0.0; 3];
        let mut signed_penetrations = [0.0; 3];
        let mut normal_directions = [0.0; 3];
        let mut separating_axes = 0;
        let mut separating_axis = 0;

        for axis in 0..3 {
            let mut min_penetration = self.max[axis] - stationary.min[axis];
            let mut max_penetration = stationary.max[axis] - self.min[axis];
            if min_penetration.abs() <= 1.0e-7 {
                min_penetration = 0.0;
            }
            if max_penetration.abs() <= 1.0e-7 {
                max_penetration = 0.0;
            }

            let min_positive = min_penetration.max(0.0);
            let max_positive = max_penetration.max(0.0);
            if min_positive == 0.0 {
                axis_penetrations[axis] = 0.0;
                signed_penetrations[axis] = min_penetration;
                normal_directions[axis] = -1.0;
                separating_axes += 1;
                separating_axis = axis;
            } else if max_positive == 0.0 {
                axis_penetrations[axis] = 0.0;
                signed_penetrations[axis] = max_penetration;
                normal_directions[axis] = 1.0;
                separating_axes += 1;
                separating_axis = axis;
            } else if min_positive < max_positive {
                axis_penetrations[axis] = min_positive;
                signed_penetrations[axis] = min_positive;
                normal_directions[axis] = -1.0;
            } else {
                axis_penetrations[axis] = max_positive;
                signed_penetrations[axis] = max_positive;
                normal_directions[axis] = 1.0;
            }

            if separating_axes > 1 {
                return velocity;
            }
        }

        if separating_axes == 0 {
            let mut best_axis = 0;
            for axis in 1..3 {
                if axis_penetrations[axis] < axis_penetrations[best_axis] {
                    best_axis = axis;
                }
            }
            let desired = axis_penetrations[best_axis] * normal_directions[best_axis];
            let mut depenetrated = velocity;
            depenetrated[best_axis] = if desired > 0.0 {
                desired.max(velocity[best_axis])
            } else {
                desired.min(velocity[best_axis])
            };
            return depenetrated;
        }

        let swept_penetration = signed_penetrations[separating_axis]
            - normal_directions[separating_axis] * velocity[separating_axis];
        if swept_penetration <= 0.0 {
            return velocity;
        }
        let mut clipped = velocity;
        clipped[separating_axis] =
            signed_penetrations[separating_axis] * normal_directions[separating_axis];
        clipped
    }
}
