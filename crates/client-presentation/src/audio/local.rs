//! Client-predicted local-player motion cues: footsteps, jump, land, swim and splash.

/// Blocks walked between footsteps; needs native measurement.
const STEP_STRIDE: f64 = 1.6;
/// Blocks swum between swim sounds; needs native measurement.
const SWIM_STRIDE: f64 = 2.0;
/// Downward speed (blocks/tick) that makes a landing audible; needs native measurement.
const LAND_MIN_SPEED: f64 = 0.15;
const TELEPORT_BLOCKS: f64 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LocalCue {
    Step,
    Jump,
    Land { speed: f32 },
    Swim,
    Splash,
}

#[derive(Clone, Copy, Debug)]
pub struct MotionSample {
    pub position: [f64; 3],
    pub velocity_y: f64,
    pub on_ground: bool,
    pub sneaking: bool,
    pub in_water: bool,
}

#[derive(Debug, Default)]
pub struct LocalMotion {
    previous: Option<MotionSample>,
    walked: f64,
}

impl LocalMotion {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Cues raised by moving from the previous sample to `sample` (one physics tick).
    pub fn advance(&mut self, sample: MotionSample) -> Vec<LocalCue> {
        let mut cues = Vec::new();
        let Some(previous) = self.previous.replace(sample) else {
            return cues;
        };
        let dx = sample.position[0] - previous.position[0];
        let dz = sample.position[2] - previous.position[2];
        let horizontal = (dx * dx + dz * dz).sqrt();
        if horizontal > TELEPORT_BLOCKS {
            self.walked = 0.0;
            return cues;
        }
        if sample.in_water && !previous.in_water && previous.velocity_y < -0.1 {
            cues.push(LocalCue::Splash);
        }
        if sample.in_water {
            self.walked += horizontal + (sample.position[1] - previous.position[1]).abs();
            if self.walked >= SWIM_STRIDE {
                self.walked = 0.0;
                cues.push(LocalCue::Swim);
            }
            return cues;
        }
        if previous.on_ground && !sample.on_ground && sample.velocity_y > 0.1 {
            cues.push(LocalCue::Jump);
        }
        if !previous.on_ground && sample.on_ground && -previous.velocity_y >= LAND_MIN_SPEED {
            cues.push(LocalCue::Land {
                speed: (-previous.velocity_y) as f32,
            });
            self.walked = 0.0;
        }
        if sample.on_ground && !sample.sneaking {
            self.walked += horizontal;
            if self.walked >= STEP_STRIDE {
                self.walked = 0.0;
                cues.push(LocalCue::Step);
            }
        } else if !sample.on_ground {
            self.walked = 0.0;
        }
        cues
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64, vy: f64, on_ground: bool) -> MotionSample {
        MotionSample {
            position: [x, y, 0.0],
            velocity_y: vy,
            on_ground,
            sneaking: false,
            in_water: false,
        }
    }

    #[test]
    fn walking_steps_once_per_stride_and_sneaking_is_silent() {
        let mut motion = LocalMotion::default();
        let mut steps = 0;
        for tick in 0..=40 {
            steps += motion
                .advance(at(f64::from(tick) * 0.5, 0.0, 0.0, true))
                .iter()
                .filter(|cue| **cue == LocalCue::Step)
                .count();
        }
        assert_eq!(steps, 10);
        motion.reset();
        for tick in 0..=40 {
            let mut sample = at(f64::from(tick) * 0.5, 0.0, 0.0, true);
            sample.sneaking = true;
            assert!(motion.advance(sample).is_empty());
        }
    }

    #[test]
    fn jump_and_land_are_edge_triggered() {
        let mut motion = LocalMotion::default();
        motion.advance(at(0.0, 0.0, 0.0, true));
        assert_eq!(motion.advance(at(0.0, 0.4, 0.42, false)), [LocalCue::Jump]);
        assert!(motion.advance(at(0.0, 0.7, 0.2, false)).is_empty());
        motion.advance(at(0.0, 0.3, -0.5, false));
        let landed = motion.advance(at(0.0, 0.0, 0.0, true));
        assert_eq!(landed, [LocalCue::Land { speed: 0.5 }]);
    }

    #[test]
    fn entering_water_splashes_and_teleports_are_ignored() {
        let mut motion = LocalMotion::default();
        motion.advance(at(0.0, 5.0, -0.4, false));
        let mut wet = at(0.0, 4.5, -0.4, false);
        wet.in_water = true;
        assert!(motion.advance(wet).contains(&LocalCue::Splash));
        assert!(motion.advance(at(100.0, 5.0, 0.0, true)).is_empty());
    }
}
