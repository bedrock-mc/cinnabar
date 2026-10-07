//! Wheel targets accumulate independently of the displayed offset; direct grabs stay immediate.

const WHEEL_SECONDS: f64 = 0.085;
const PIXEL_SECONDS: f64 = 0.035;

pub(super) struct ScrollTween {
    from: f32,
    pub(super) target: f32,
    started: f64,
    duration: f64,
}

impl ScrollTween {
    pub(super) fn new(from: f32, target: f32, pixels: bool, seconds: f64) -> Self {
        Self {
            from,
            target,
            started: seconds,
            duration: if pixels { PIXEL_SECONDS } else { WHEEL_SECONDS },
        }
    }

    pub(super) fn sample(&self, seconds: f64) -> f32 {
        let t = ((seconds - self.started) / self.duration).clamp(0.0, 1.0) as f32;
        self.from + (self.target - self.from) * (1.0 - (1.0 - t).powi(3))
    }

    pub(super) fn retarget(&mut self, target: f32, pixels: bool, seconds: f64) {
        if target != self.target {
            *self = Self::new(self.sample(seconds), target, pixels, seconds);
        }
    }

    pub(super) fn settled(&self, seconds: f64) -> bool {
        seconds - self.started >= self.duration
    }

    pub(super) fn clamp(&mut self, max: f32) {
        self.from = self.from.clamp(0.0, max);
        self.target = self.target.clamp(0.0, max);
    }
}
