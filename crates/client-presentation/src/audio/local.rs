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
    Swim { volume: f32 },
    Splash { volume: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct MotionSample {
    pub position: [f64; 3],
    pub velocity_y: f64,
    /// Tick-start velocity before liquid travel changes the impact speed.
    pub entry_velocity: [f32; 3],
    pub movement: [f32; 3],
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
        if sample.in_water && !previous.in_water {
            cues.push(LocalCue::Splash {
                volume: super::water::motion_volume(
                    sample.entry_velocity,
                    super::water::SPLASH_SCALE,
                ),
            });
        }
        if sample.in_water {
            self.walked += horizontal + (sample.position[1] - previous.position[1]).abs();
            if self.walked >= SWIM_STRIDE {
                self.walked = 0.0;
                cues.push(LocalCue::Swim {
                    volume: super::water::motion_volume(sample.movement, super::water::SWIM_SCALE),
                });
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
#[path = "local/tests.rs"]
mod tests;
