use serde::{Deserialize, Serialize};

use crate::Vec3;

pub const PLAYER_WIDTH: f64 = 0.6;
pub const PLAYER_HEIGHT: f64 = 1.8;

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
        Self::player_with_height_at(feet, PLAYER_HEIGHT)
    }

    /// Player box with a pose-dependent height (sneaking, swimming, crawling).
    #[must_use]
    pub fn player_with_height_at(feet: Vec3, height: f64) -> Self {
        // Vanilla builds and retains full-width faces.
        let half_width = PLAYER_WIDTH as f32 * 0.5;
        let [x, y, z] = [feet.x as f32, feet.y as f32, feet.z as f32];
        Self::new(
            Vec3::new(
                f64::from(x - half_width),
                f64::from(y),
                f64::from(z - half_width),
            ),
            Vec3::new(
                f64::from(x + half_width),
                f64::from(y + height as f32),
                f64::from(z + half_width),
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
        (self.max.x as f32) > (rhs.min.x as f32)
            && (self.min.x as f32) < (rhs.max.x as f32)
            && (self.max.y as f32) > (rhs.min.y as f32)
            && (self.min.y as f32) < (rhs.max.y as f32)
            && (self.max.z as f32) > (rhs.min.z as f32)
            && (self.min.z as f32) < (rhs.max.z as f32)
    }

    /// Fraction of the closed segment `origin..origin + delta` at which it first touches this box.
    #[must_use]
    pub fn segment_entry(self, origin: Vec3, delta: Vec3) -> Option<f64> {
        let mut entry = 0.0_f64;
        let mut exit = 1.0_f64;
        for axis in 0..3 {
            if delta[axis].abs() <= f64::EPSILON {
                if origin[axis] < self.min[axis] || origin[axis] > self.max[axis] {
                    return None;
                }
                continue;
            }
            let first = (self.min[axis] - origin[axis]) / delta[axis];
            let second = (self.max[axis] - origin[axis]) / delta[axis];
            entry = entry.max(first.min(second));
            exit = exit.min(first.max(second));
            if entry > exit {
                return None;
            }
        }
        (exit >= 0.0 && entry <= 1.0).then_some(entry.clamp(0.0, 1.0))
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
            let mut min_penetration = self.max[axis] as f32 - stationary.min[axis] as f32;
            let mut max_penetration = stationary.max[axis] as f32 - self.min[axis] as f32;
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
                f64::from(desired.max(velocity[best_axis] as f32))
            } else {
                f64::from(desired.min(velocity[best_axis] as f32))
            };
            return depenetrated;
        }

        let swept_penetration = signed_penetrations[separating_axis]
            - normal_directions[separating_axis] * velocity[separating_axis] as f32;
        if swept_penetration <= 0.0 {
            return velocity;
        }
        let mut clipped = velocity;
        clipped[separating_axis] =
            f64::from(signed_penetrations[separating_axis] * normal_directions[separating_axis]);
        clipped
    }

    /// Pure minimal-translation vector separating two fully overlapping
    /// boxes along their smallest penetration axis, or `None` when they are
    /// already separated under exactly [`Self::clip_against`]'s epsilon
    /// rules (touching counts as separated).
    ///
    /// Mirrors the fully-overlapped branch of [`Self::clip_against`]; kept
    /// deliberately separate so the pinned swept-clip byte behavior cannot
    /// drift with recovery-policy evolution.
    #[must_use]
    pub fn overlap_minimal_translation(self, stationary: Self) -> Option<Vec3> {
        if stationary.is_zero_volume() {
            return None;
        }

        let mut axis_penetrations = [0.0; 3];
        let mut normal_directions = [0.0; 3];
        for axis in 0..3 {
            let mut min_penetration = self.max[axis] as f32 - stationary.min[axis] as f32;
            let mut max_penetration = stationary.max[axis] as f32 - self.min[axis] as f32;
            if min_penetration.abs() <= 1.0e-7 {
                min_penetration = 0.0;
            }
            if max_penetration.abs() <= 1.0e-7 {
                max_penetration = 0.0;
            }
            let min_positive = min_penetration.max(0.0);
            let max_positive = max_penetration.max(0.0);
            if min_positive == 0.0 || max_positive == 0.0 {
                // A single separated axis already proves non-overlap; the
                // swept branches of `clip_against` own that case instead.
                return None;
            }
            if min_positive < max_positive {
                axis_penetrations[axis] = min_positive;
                normal_directions[axis] = -1.0;
            } else {
                axis_penetrations[axis] = max_positive;
                normal_directions[axis] = 1.0;
            }
        }

        let mut best_axis = 0;
        for axis in 1..3 {
            if axis_penetrations[axis] < axis_penetrations[best_axis] {
                best_axis = axis;
            }
        }
        let mut translation = Vec3::ZERO;
        translation[best_axis] =
            f64::from(axis_penetrations[best_axis] * normal_directions[best_axis]);
        Some(translation)
    }
}

/// PROVISIONAL bounded recovery policy for spawn anchors installed inside
/// solids — explicitly NOT a vanilla-parity claim and not part of the pinned
/// tick equations.
///
/// Iteratively applies each overlapping collider's
/// [`Aabb::overlap_minimal_translation`] to the `height`-tall player box until no
/// collider overlaps, [`max_iterations`] is exhausted, or the net feet
/// displacement would exceed `max_displacement_blocks`. Returns the adjusted
/// feet origin only when the final box is provably clear under the same
/// epsilon rules as [`Aabb::clip_against`]; `None` lets the caller fail
/// closed instead of transmitting depenetration garbage.
#[must_use]
pub fn depenetrate_player(
    feet: Vec3,
    height: f64,
    colliders: &[Aabb],
    max_iterations: usize,
    max_displacement_blocks: f64,
) -> Option<Vec3> {
    if !feet.is_finite() || !max_displacement_blocks.is_finite() {
        return None;
    }
    let origin = feet;
    let mut feet = feet;
    for _ in 0..max_iterations {
        let player = Aabb::player_with_height_at(feet, height);
        let mut deepest: Option<(f64, Vec3)> = None;
        for collider in colliders.iter().copied() {
            if collider.is_zero_volume() || !collider.min.is_finite() || !collider.max.is_finite() {
                continue;
            }
            let Some(translation) = player.overlap_minimal_translation(collider) else {
                continue;
            };
            let magnitude_squared = translation.length_squared();
            if magnitude_squared == 0.0 {
                continue;
            }
            if deepest.is_none_or(|(best, _)| magnitude_squared < best) {
                deepest = Some((magnitude_squared, translation));
            }
        }
        let Some((_, translation)) = deepest else {
            // No collider overlaps this box: provably clear.
            return Some(feet);
        };
        feet += translation;
        if !feet.is_finite()
            || (feet - origin).length_squared() > max_displacement_blocks * max_displacement_blocks
        {
            return None;
        }
    }
    let player = Aabb::player_with_height_at(feet, height);
    colliders
        .iter()
        .all(|collider| {
            collider.is_zero_volume() || player.overlap_minimal_translation(*collider).is_none()
        })
        .then_some(feet)
}

#[cfg(test)]
mod tests {
    use super::{Aabb, Vec3};

    #[test]
    fn overlap_translation_matches_the_clip_against_overlap_branch() {
        let fixtures: &[(&str, Aabb, Aabb)] = &[
            (
                "feet inside one block",
                Aabb::new(
                    Vec3::new(-0.2999, 0.5, -0.2999),
                    Vec3::new(0.2999, 2.3, 0.2999),
                ),
                Aabb::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)),
            ),
            (
                "off-center embedment",
                Aabb::new(Vec3::new(0.2, 0.1, 0.2), Vec3::new(0.9, 1.4, 0.95)),
                Aabb::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)),
            ),
            (
                "deep box embedment",
                Aabb::new(Vec3::new(-5.0, -5.0, -5.0), Vec3::new(5.0, 5.0, 5.0)),
                Aabb::new(Vec3::new(-6.0, -6.0, -6.0), Vec3::new(6.0, 6.0, 6.0)),
            ),
        ];
        for (name, moving, stationary) in fixtures {
            let translation = moving
                .overlap_minimal_translation(*stationary)
                .unwrap_or_else(|| panic!("{name} must report full overlap"));
            assert_eq!(
                translation,
                moving.clip_against(*stationary, Vec3::ZERO),
                "{name}: the pure query must agree with the pinned clip branch",
            );
        }
    }

    #[test]
    fn touching_and_separated_boxes_report_no_overlap_translation() {
        let floor = Aabb::new(Vec3::new(-64.0, -1.0, -64.0), Vec3::new(64.0, 0.0, 64.0));
        let resting = Aabb::player_at(Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(resting.overlap_minimal_translation(floor), None);
        let airborne = Aabb::player_at(Vec3::new(0.0, 0.25, 0.0));
        assert_eq!(airborne.overlap_minimal_translation(floor), None);
        let zero_volume = Aabb::new(Vec3::ONE, Vec3::ONE);
        assert_eq!(resting.overlap_minimal_translation(zero_volume), None);
    }
}
