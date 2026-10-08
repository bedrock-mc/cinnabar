//! Cinematic camera paths: keyframes in Bedrock angles, eased between, sampled on game time.

use serde::{Deserialize, Serialize};

use crate::input::yaw_difference;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    #[default]
    EaseInOut,
    /// Holds the previous keyframe until the next one is reached.
    Step,
}

impl Easing {
    /// Maps linear progress in 0..=1 to eased progress in 0..=1.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t * t,
            Self::EaseOut => 1.0 - (1.0 - t).powi(3),
            Self::EaseInOut => t * t * (3.0 - 2.0 * t),
            Self::Step => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    /// Seconds of game time after the path starts.
    pub t: f32,
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// Vertical field of view in degrees; omitted keeps the player's.
    #[serde(default)]
    pub fov: Option<f32>,
    /// Easing into this keyframe from the previous one; defaults to the path's.
    #[serde(default)]
    pub easing: Option<Easing>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraPath {
    pub keyframes: Vec<Keyframe>,
    #[serde(default)]
    pub easing: Easing,
    /// Restart from the first keyframe after the last.
    #[serde(default, rename = "loop")]
    pub looping: bool,
    /// Hide the first-person hand while the path owns the camera.
    #[serde(default = "hide_hand_default")]
    pub hide_hand: bool,
}

fn hide_hand_default() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraSample {
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub fov: Option<f32>,
}

impl CameraPath {
    /// Keyframes must be finite and strictly increasing in time.
    pub fn validate(&self) -> Result<(), String> {
        if self.keyframes.is_empty() {
            return Err("a camera path needs at least one keyframe".into());
        }
        for frame in &self.keyframes {
            let finite = frame.t.is_finite()
                && frame.position.iter().all(|axis| axis.is_finite())
                && frame.yaw.is_finite()
                && frame.pitch.is_finite()
                && frame
                    .fov
                    .is_none_or(|fov| fov.is_finite() && fov > 0.0 && fov < 180.0);
            if !finite || frame.t < 0.0 {
                return Err(format!("keyframe at t={} has invalid values", frame.t));
            }
        }
        if self.keyframes.windows(2).any(|pair| pair[1].t <= pair[0].t) {
            return Err("keyframe times must strictly increase".into());
        }
        Ok(())
    }

    /// Game seconds until the last keyframe.
    pub fn duration(&self) -> f32 {
        self.keyframes.last().map_or(0.0, |frame| frame.t)
    }

    /// Whether a non-looping path has played out at `elapsed`.
    pub fn finished(&self, elapsed: f32) -> bool {
        !self.looping && elapsed >= self.duration()
    }

    pub fn sample(&self, elapsed: f32) -> Option<CameraSample> {
        let first = self.keyframes.first()?;
        let duration = self.duration();
        let elapsed = if self.looping && duration > 0.0 {
            elapsed.rem_euclid(duration)
        } else {
            elapsed
        };
        let next = self.keyframes.iter().position(|frame| frame.t > elapsed);
        let (from, to) = match next {
            None => {
                let last = self.keyframes.last()?;
                return Some(at(last, last, 0.0));
            }
            Some(0) => return Some(at(first, first, 0.0)),
            Some(index) => (&self.keyframes[index - 1], &self.keyframes[index]),
        };
        let progress = (elapsed - from.t) / (to.t - from.t);
        let eased = to.easing.unwrap_or(self.easing).apply(progress);
        Some(at(from, to, eased))
    }
}

fn at(from: &Keyframe, to: &Keyframe, t: f32) -> CameraSample {
    let lerp = |a: f32, b: f32| a + (b - a) * t;
    CameraSample {
        position: std::array::from_fn(|axis| lerp(from.position[axis], to.position[axis])),
        yaw: from.yaw + yaw_difference(from.yaw, to.yaw) * t,
        pitch: lerp(from.pitch, to.pitch),
        fov: match (from.fov, to.fov) {
            (Some(a), Some(b)) => Some(lerp(a, b)),
            (one, other) => other.or(one),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(t: f32, x: f32, yaw: f32) -> Keyframe {
        Keyframe {
            t,
            position: [x, 64.0, 0.0],
            yaw,
            pitch: 0.0,
            fov: None,
            easing: None,
        }
    }

    #[test]
    fn linear_paths_interpolate_and_clamp() {
        let path = CameraPath {
            keyframes: vec![frame(0.0, 0.0, 0.0), frame(2.0, 10.0, 90.0)],
            easing: Easing::Linear,
            looping: false,
            hide_hand: true,
        };
        path.validate().unwrap();
        let mid = path.sample(1.0).unwrap();
        assert_eq!(mid.position[0], 5.0);
        assert_eq!(mid.yaw, 45.0);
        assert_eq!(path.sample(5.0).unwrap().position[0], 10.0);
        assert!(path.finished(2.0) && !path.finished(1.9));
    }

    #[test]
    fn easing_is_monotonic_with_fixed_ends() {
        for easing in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            assert_eq!(easing.apply(0.0), 0.0);
            assert_eq!(easing.apply(1.0), 1.0);
            let samples: Vec<f32> = (0..=20).map(|i| easing.apply(i as f32 / 20.0)).collect();
            assert!(
                samples.windows(2).all(|pair| pair[1] >= pair[0]),
                "{easing:?}"
            );
        }
        assert_eq!(Easing::EaseInOut.apply(0.5), 0.5);
        assert_eq!(Easing::Step.apply(0.99), 0.0);
    }

    #[test]
    fn yaw_wraps_the_short_way_and_paths_loop() {
        let path = CameraPath {
            keyframes: vec![frame(0.0, 0.0, 350.0), frame(1.0, 0.0, 10.0)],
            easing: Easing::Linear,
            looping: true,
            hide_hand: true,
        };
        assert_eq!(path.sample(0.5).unwrap().yaw, 360.0);
        assert_eq!(path.sample(1.5).unwrap().yaw, 360.0);
        assert!(!path.finished(100.0));
    }

    #[test]
    fn invalid_paths_are_rejected() {
        let unordered = CameraPath {
            keyframes: vec![frame(1.0, 0.0, 0.0), frame(1.0, 1.0, 0.0)],
            easing: Easing::Linear,
            looping: false,
            hide_hand: true,
        };
        assert!(unordered.validate().is_err());
        let empty = CameraPath {
            keyframes: Vec::new(),
            easing: Easing::Linear,
            looping: false,
            hide_hand: true,
        };
        assert!(empty.validate().is_err());
    }
}
