//! Piecewise fade alpha with overlapping instructions and a stable active color.

use protocol::CameraFadeInstruction;

const MAX_FADE_KEYFRAMES: usize = 1024;
const KEYFRAME_EPSILON: f32 = 1.0e-4;

#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct FadeState {
    keyframes: Vec<[f32; 2]>,
    color: [f32; 3],
    elapsed: f32,
    alpha: f32,
}

impl FadeState {
    /// Adds an overlapping envelope, preserving the color until the current animation ends.
    pub(super) fn apply(&mut self, fade: &CameraFadeInstruction) -> bool {
        let [fade_in, hold, fade_out] = fade.time.map_or([1.0, 0.5, 1.0], |time| {
            [
                time.fade_in_seconds,
                time.hold_seconds,
                time.fade_out_seconds,
            ]
        });
        if [fade_in, hold, fade_out]
            .iter()
            .any(|time| !time.is_finite() || *time < 0.0)
            || !(fade_in + hold + fade_out).is_finite()
            || self.keyframes.len() + 4 > MAX_FADE_KEYFRAMES
        {
            return false;
        }
        if self.keyframes.is_empty() {
            self.color = fade.color.map_or([0.0; 3], |color| {
                [color.red, color.green, color.blue].map(|channel| channel.clamp(0.0, 1.0))
            });
        } else {
            for frame in &mut self.keyframes {
                frame[0] -= self.elapsed;
            }
            self.keyframes.retain(|frame| frame[0] >= 0.0);
            self.keyframes.insert(0, [0.0, self.alpha]);
        }
        self.elapsed = 0.0;
        let hold_end = fade_in + hold.max(0.5 - fade_in - fade_out);
        self.add_keyframe(fade_in, 1.0);
        self.add_keyframe(hold_end, 1.0);
        self.add_keyframe(hold_end + fade_out, 0.0);
        self.alpha = self.evaluate(0.0);
        true
    }

    /// Advances the envelope without allocating or mutating its keyframe layout.
    pub(super) fn advance(&mut self, delta_seconds: f32) {
        if self.keyframes.is_empty() {
            return;
        }
        self.elapsed += delta_seconds;
        if self.elapsed >= self.keyframes.last().unwrap()[0] {
            self.keyframes.clear();
            self.elapsed = 0.0;
            self.alpha = 0.0;
        } else {
            self.alpha = self.evaluate(self.elapsed);
        }
    }

    /// Returns an overlay only while the animation has retained keyframes.
    pub(super) fn overlay(&self) -> Option<([f32; 3], f32)> {
        (!self.keyframes.is_empty()).then_some((self.color, self.alpha))
    }

    /// Inserts only points above the previous envelope, or after its final point.
    fn add_keyframe(&mut self, time: f32, alpha: f32) {
        if self.evaluate(time) + KEYFRAME_EPSILON >= alpha
            && self
                .keyframes
                .last()
                .is_some_and(|last| time <= last[0] + KEYFRAME_EPSILON)
        {
            return;
        }
        let index = self
            .keyframes
            .partition_point(|frame| frame[0] < time - KEYFRAME_EPSILON);
        if let Some(frame) = self.keyframes.get_mut(index)
            && (frame[0] - time).abs() <= KEYFRAME_EPSILON
        {
            frame[1] = alpha;
        } else {
            self.keyframes.insert(index, [time, alpha]);
        }
        let mut index = 1;
        while index + 1 < self.keyframes.len() {
            let alpha = self.keyframes[index][1];
            if self.keyframes[index - 1][1] >= alpha && self.keyframes[index + 1][1] >= alpha {
                self.keyframes.remove(index);
            } else {
                index += 1;
            }
        }
    }

    /// Interpolates from transparent at time zero, with native tolerance at keyframe boundaries.
    fn evaluate(&self, time: f32) -> f32 {
        if time < 0.0 || self.keyframes.last().is_none_or(|last| time > last[0]) {
            return 0.0;
        }
        let mut previous = [0.0, 0.0];
        for &next in &self.keyframes {
            if (time - next[0]).abs() <= KEYFRAME_EPSILON {
                return next[1];
            }
            if time < next[0] {
                let t = (time - previous[0]) / (next[0] - previous[0]);
                return previous[1] + (next[1] - previous[1]) * t;
            }
            previous = next;
        }
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{CameraFadeColor, CameraFadeTimes};

    /// Constructs an explicitly timed fade with optional red color.
    fn fade(times: [f32; 3], red: bool) -> CameraFadeInstruction {
        CameraFadeInstruction {
            time: Some(CameraFadeTimes {
                fade_in_seconds: times[0],
                hold_seconds: times[1],
                fade_out_seconds: times[2],
            }),
            color: red.then_some(CameraFadeColor {
                red: 1.0,
                green: 0.0,
                blue: 0.0,
            }),
        }
    }

    #[test]
    fn missing_timing_uses_defaults_and_duration_has_half_second_floor() {
        let mut state = FadeState::default();
        assert!(state.apply(&CameraFadeInstruction {
            time: None,
            color: None
        }));
        state.advance(0.5);
        assert_eq!(state.overlay(), Some(([0.0; 3], 0.5)));
        state.advance(2.0);
        assert_eq!(state.overlay(), None);
        state.apply(&fade([0.0; 3], false));
        assert_eq!(state.overlay(), Some(([0.0; 3], 1.0)));
        state.advance(0.25);
        assert_eq!(state.overlay(), Some(([0.0; 3], 1.0)));
        state.advance(0.25);
        assert_eq!(state.overlay(), None);
    }

    #[test]
    fn overlapping_fades_preserve_current_alpha_and_first_color() {
        let mut state = FadeState::default();
        state.apply(&fade([1.0, 0.0, 1.0], true));
        state.advance(0.5);
        state.apply(&fade([1.0, 1.0, 1.0], false));
        assert_eq!(state.overlay(), Some(([1.0, 0.0, 0.0], 0.5)));
        state.advance(0.5);
        assert_eq!(state.overlay(), Some(([1.0, 0.0, 0.0], 1.0)));
    }

    #[test]
    fn negative_timing_is_skipped_without_replacing_active_fade() {
        let mut state = FadeState::default();
        state.apply(&fade([1.0; 3], true));
        let before = state.clone();
        assert!(!state.apply(&fade([-1.0, 0.0, 0.0], false)));
        assert_eq!(state, before);
    }
}
