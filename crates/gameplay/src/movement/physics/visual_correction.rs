//! Tick-sampled correction offsets applied only to the presented player position.

use sim::Vec3;

const MAX_OFFSET: f32 = 4.0;
const SPEED_SQUARED_FACTOR: f32 = 0.2;
const MIN_DIRECTION_LENGTH: f32 = 0.0001;

#[derive(Debug, Default, Clone, Copy)]
pub(super) struct VisualCorrection {
    previous: Vec3,
    current: Vec3,
    direction: Vec3,
    speed_squared: f32,
    falling_motion: Vec3,
}

impl VisualCorrection {
    /// Holds the completed correction offset while movement cannot advance.
    pub(super) fn finish_interpolation(&mut self) {
        self.previous = self.current;
    }

    /// Accumulates a bounded render offset without changing movement authority.
    pub(super) fn correct(&mut self, current: Vec3, previous: Vec3, motion: Vec3) {
        self.current = bounded(self.current + current);
        self.previous = bounded(self.previous + previous);
        let squared = self.current.length_squared() as f32;
        let length = squared.sqrt();
        self.direction = if length >= MIN_DIRECTION_LENGTH {
            self.current * f64::from(1.0 / length)
        } else {
            Vec3::ZERO
        };
        self.speed_squared = squared * SPEED_SQUARED_FACTOR;
        if self.current.y <= 0.0 {
            self.speed_squared = self.speed_squared.max(motion.length_squared() as f32);
        } else {
            self.falling_motion = motion;
        }
    }

    /// Advances offset decay once per successful simulation tick.
    pub(super) fn tick(&mut self) {
        self.previous = self.current;
        let mut speed_squared = self.speed_squared;
        if self.current.y > 0.0 {
            self.falling_motion.y =
                f64::from(self.falling_motion.y as f32 - sim::NORMAL_GRAVITY as f32);
            speed_squared = speed_squared.max(self.falling_motion.length_squared() as f32);
        }
        let squared = self.current.length_squared() as f32;
        if squared <= speed_squared {
            *self = Self::default();
        } else {
            self.current = self.direction * f64::from(squared.sqrt() - speed_squared.sqrt());
        }
    }

    /// Samples the retained correction at the same fraction as the player pose.
    pub(super) fn offset(&self, alpha: f32) -> Vec3 {
        self.previous + (self.current - self.previous) * f64::from(alpha.clamp(0.0, 1.0))
    }
}

/// Keeps each retained render sample inside the correction radius.
fn bounded(offset: Vec3) -> Vec3 {
    let squared = offset.length_squared() as f32;
    if squared > MAX_OFFSET * MAX_OFFSET {
        offset * f64::from(MAX_OFFSET / squared.sqrt())
    } else {
        offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_correction_decays_on_ticks_and_interpolates_between_them() {
        let mut offset = VisualCorrection::default();
        offset.correct(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::ZERO,
        );
        offset.tick();
        let step = (4.0_f32 * SPEED_SQUARED_FACTOR).sqrt();
        assert_eq!(offset.offset(0.0).x, 2.0);
        assert!((offset.offset(0.5).x as f32 - (2.0 - step / 2.0)).abs() < 1.0e-6);
        offset.tick();
        offset.tick();
        assert_eq!(offset.offset(0.0), Vec3::ZERO);
        assert_eq!(offset.offset(1.0), Vec3::ZERO);
    }

    #[test]
    fn repeated_corrections_accumulate_with_a_bounded_offset() {
        let mut offset = VisualCorrection::default();
        offset.correct(Vec3::new(3.0, 0.0, 0.0), Vec3::ZERO, Vec3::ZERO);
        offset.correct(Vec3::new(3.0, 0.0, 0.0), Vec3::ZERO, Vec3::ZERO);
        assert_eq!(
            offset.offset(1.0),
            Vec3::new(f64::from(MAX_OFFSET), 0.0, 0.0)
        );
    }

    #[test]
    fn oversized_corrections_bound_both_interpolation_samples() {
        let mut offset = VisualCorrection::default();
        for _ in 0..2 {
            offset.correct(
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(-10.0, 0.0, 0.0),
                Vec3::ZERO,
            );
            assert_eq!(
                offset.offset(0.0),
                Vec3::new(-f64::from(MAX_OFFSET), 0.0, 0.0)
            );
            assert_eq!(
                offset.offset(1.0),
                Vec3::new(f64::from(MAX_OFFSET), 0.0, 0.0)
            );
            for alpha in [0.0, 0.25, 0.5, 0.75, 1.0] {
                assert!(
                    offset.offset(alpha).length_squared() <= f64::from(MAX_OFFSET * MAX_OFFSET)
                );
            }
        }
        offset.tick();
        assert_eq!(
            offset.offset(0.0),
            Vec3::new(f64::from(MAX_OFFSET), 0.0, 0.0)
        );
        assert!(offset.offset(1.0).length_squared() < offset.offset(0.0).length_squared());
    }
}
