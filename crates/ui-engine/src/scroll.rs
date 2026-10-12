//! Scroll views as drawn and their wheel motion. Wheel targets accumulate independently of the
//! displayed offset; direct grabs stay immediate.

use ui::{UiPoint, UiRect};

const WHEEL_SECONDS: f64 = 0.085;
const PIXEL_SECONDS: f64 = 0.035;

/// A wheel scroll easing from the displayed offset toward its accumulated target.
pub struct ScrollTween {
    from: f32,
    pub target: f32,
    started: f64,
    duration: f64,
}

impl ScrollTween {
    pub fn new(from: f32, target: f32, pixels: bool, seconds: f64) -> Self {
        Self {
            from,
            target,
            started: seconds,
            duration: if pixels { PIXEL_SECONDS } else { WHEEL_SECONDS },
        }
    }

    pub fn sample(&self, seconds: f64) -> f32 {
        let t = ((seconds - self.started) / self.duration).clamp(0.0, 1.0) as f32;
        self.from + (self.target - self.from) * (1.0 - (1.0 - t).powi(3))
    }

    pub fn retarget(&mut self, target: f32, pixels: bool, seconds: f64) {
        if target != self.target {
            *self = Self::new(self.sample(seconds), target, pixels, seconds);
        }
    }

    pub fn settled(&self, seconds: f64) -> bool {
        seconds - self.started >= self.duration
    }

    pub fn clamp(&mut self, max: f32) {
        self.from = self.from.clamp(0.0, max);
        self.target = self.target.clamp(0.0, max);
    }
}

/// One scroll view as drawn; offsets are in the drawing system's own units.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollArea {
    pub key: String,
    pub viewport: UiRect,
    /// Window-logical pixels per offset unit.
    pub scale: f32,
    pub offset: f32,
    pub max: f32,
    /// Offset units per wheel notch.
    pub speed: f32,
    pub track: Option<UiRect>,
    pub thumb: Option<UiRect>,
    /// A JSON-UI view's metrics and the window point of its virtual origin: its
    /// input follows the client's scroll rules.
    pub engine: Option<(json_ui::ScrollMetrics, [f32; 2])>,
    /// Whether the box can be grabbed (its `draggable` is not `not_draggable`).
    pub draggable: bool,
}

impl ScrollArea {
    /// The engine metrics at the current offset, and `point` in virtual pixels.
    pub fn engine_at(&self, point: UiPoint) -> Option<(json_ui::ScrollMetrics, [f64; 2])> {
        let (metrics, origin) = self.engine.as_ref()?;
        let metrics = json_ui::ScrollMetrics {
            offset: f64::from(self.offset),
            ..metrics.clone()
        };
        let virtual_at = |value: f32, axis: usize| f64::from((value - origin[axis]) / self.scale);
        Some((
            metrics,
            [virtual_at(point.x(), 0), virtual_at(point.y(), 1)],
        ))
    }

    /// The offset that puts the thumb's top at window `y`.
    pub fn offset_for_thumb(&self, y: f32) -> f32 {
        let (Some(track), Some(thumb)) = (self.track, self.thumb) else {
            return self.offset;
        };
        let travel = track.height() - thumb.height();
        if travel <= 0.0 {
            return self.offset;
        }
        ((y - track.min().y()) / travel * self.max).clamp(0.0, self.max)
    }
}
