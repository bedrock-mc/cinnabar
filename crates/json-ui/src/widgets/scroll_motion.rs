//! Touch scrolling as 1.26.50 runs it: a held finger pulls the offset on a spring, a release flings it
//! with friction, and past either end a rubber band pulls it back. A touch-mode
//! scrollbar box shows on touch and fades out a second after it settles.

use crate::state::{LayoutReport, ScrollMetrics, ScrollRetained, ViewState};
use crate::widgets::scroll::OVERSCROLL;

/// Longest frame the integrator takes, and its fixed substep.
const MAX_STEP: f64 = 0.25;
const SUBSTEP: f64 = 0.01;
/// Window the finger's velocity is sampled over.
const SAMPLE: f64 = 0.05;
/// In-bounds deceleration (px/s²).
const FRICTION: f64 = 200.0;
/// Rubber band past an end: stiffness and damping.
const BAND_STIFFNESS: f64 = 300.0;
const BAND_DAMPING: f64 = -34.641_018;
/// A held finger's pull: position stiffness and velocity damping.
const FOLLOW_STIFFNESS: f64 = 1000.0;
const FOLLOW_DAMPING: f64 = 63.245_552;
const MAX_VELOCITY: f64 = 2000.0;
/// Drag distance past which a touch no longer counts as a press.
const TAP_SLOP: f64 = 5.0;
/// A released motion stops once this slow (px/s) and this near its bound (px).
const SETTLE_SPEED: f64 = 1.0;
const SETTLE_DISTANCE: f64 = 0.1;
/// A fading box hides only while slower than this.
const FADE_SPEED: f64 = 1.0;

/// A touch drag or fling on one scroll view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrollMotion {
    pub touching: bool,
    pub position: f64,
    pub velocity: f64,
    /// Where the finger holds the content.
    target: f64,
    /// Offset the finger moved since the last step.
    pending: f64,
    sample_sum: f64,
    sample_time: f64,
    previous: f64,
    latest: f64,
    released: bool,
    /// Total finger travel this touch.
    pub distance: f64,
}

impl ViewState {
    /// A touch lands on the view (its `scrollbar_touch_button` goes down).
    pub fn begin_scroll_touch(&mut self, key: &str, metrics: &ScrollMetrics) {
        let offset = self.scroll_offset(key);
        let retained = self.scroll_state.entry(key.to_owned()).or_default();
        if metrics.touch_mode {
            retained.bar_fade = Some(1.0);
        }
        let motion = retained.motion.get_or_insert_with(|| ScrollMotion {
            position: offset,
            target: offset,
            ..ScrollMotion::default()
        });
        if !motion.touching {
            *motion = ScrollMotion {
                touching: true,
                position: motion.position,
                target: motion.position,
                velocity: motion.velocity,
                ..ScrollMotion::default()
            };
        }
    }

    /// The held finger moved by `delta` window-virtual pixels; only a view with
    /// gesture control pans, and one whose content fits only when allowed.
    pub fn scroll_touch_moved(&mut self, key: &str, metrics: &ScrollMetrics, delta: [f64; 2]) {
        let fits = metrics.max_offset() <= 0.0;
        if !metrics.gesture || (fits && !metrics.allow_scroll_when_fits) {
            return;
        }
        let Some(motion) = self
            .scroll_state
            .get_mut(key)
            .and_then(|retained| retained.motion.as_mut())
            .filter(|motion| motion.touching)
        else {
            return;
        };
        let along = if metrics.horizontal {
            delta[0]
        } else {
            delta[1]
        };
        motion.pending -= along;
        motion.distance += along.abs();
    }

    /// The touch lifts; `true` when it was a press rather than a drag.
    pub fn end_scroll_touch(&mut self, key: &str) -> bool {
        let Some(motion) = self
            .scroll_state
            .get_mut(key)
            .and_then(|retained| retained.motion.as_mut())
        else {
            return true;
        };
        if motion.touching {
            motion.released = true;
        }
        motion.touching = false;
        motion.distance <= TAP_SLOP
    }

    /// Advance every touch motion and touch-mode box fade by `dt` seconds
    /// against the last layout; `true` when an offset or fade changed.
    pub fn step_scrolls(&mut self, report: &LayoutReport, dt: f64) -> bool {
        let mut changed = false;
        for (key, retained) in &mut self.scroll_state {
            let Some(metrics) = report.scrolls.get(key) else {
                continue;
            };
            if let Some(motion) = retained.motion.as_mut() {
                motion.step(dt, metrics.max_offset(), metrics.viewport * OVERSCROLL);
                let settled = motion.settled(metrics.max_offset());
                let offset = motion.position;
                if self.scroll.get(key) != Some(&offset) {
                    self.scroll.insert(key.clone(), offset);
                    changed = true;
                }
                if settled {
                    retained.motion = None;
                    changed = true;
                }
            }
            changed |= fade(retained, dt);
        }
        changed
    }
}

/// Fade a released touch box; `true` when its alpha moved.
fn fade(retained: &mut ScrollRetained, dt: f64) -> bool {
    let still = retained
        .motion
        .as_ref()
        .is_none_or(|motion| !motion.touching && motion.velocity.abs() <= FADE_SPEED);
    match retained.bar_fade {
        Some(alpha) if still && alpha > 0.0 => {
            let next = alpha - dt as f32;
            retained.bar_fade = Some(if next > 0.0 { next } else { 0.0 });
            true
        }
        _ => false,
    }
}

impl ScrollMotion {
    /// Released, all but still and back in bounds: snap to the bound and stop.
    fn settled(&mut self, max: f64) -> bool {
        let clamped = self.position.clamp(0.0, max);
        let rests = !self.touching
            && self.velocity.abs() < SETTLE_SPEED
            && (self.position - clamped).abs() < SETTLE_DISTANCE;
        if rests {
            self.position = clamped;
        }
        rests
    }

    fn step(&mut self, dt: f64, max: f64, slack: f64) {
        let frame = dt.min(MAX_STEP);
        let pending = self.pending;
        let (earlier, later, window);
        if frame >= SAMPLE {
            earlier = self.latest;
            later = pending;
            self.previous = self.latest;
            self.latest = pending;
            self.sample_time = SAMPLE;
            window = SAMPLE;
        } else if frame + self.sample_time <= SAMPLE {
            self.sample_sum += pending;
            self.sample_time += frame;
            earlier = self.previous;
            later = self.latest;
            window = self.sample_time;
        } else {
            let head = SAMPLE - self.sample_time;
            let share = head / SAMPLE;
            earlier = self.latest;
            later = pending * share + self.sample_sum;
            self.previous = self.latest;
            self.latest = later;
            self.sample_sum = (1.0 - share) * pending;
            self.sample_time = frame - head;
            window = self.sample_time;
        }
        let blend = window / SAMPLE;
        let estimate = ((1.0 - blend) * earlier + later * blend) / SAMPLE;
        self.target += frame * estimate;
        let mut follow = estimate;
        if self.released {
            let flick = later / SAMPLE;
            let larger = if flick < 0.0 {
                flick < estimate
            } else {
                estimate < flick
            };
            follow = if larger { flick } else { estimate };
            self.velocity = follow;
            self.sample_sum = 0.0;
            self.sample_time = 0.0;
            self.previous = 0.0;
            self.latest = 0.0;
            self.released = false;
        }
        self.pending = 0.0;
        let (hard_low, hard_high) = (-slack, max + slack);
        let mut remaining = frame;
        while remaining > 0.0 {
            let h = remaining.min(SUBSTEP);
            let (x, v) = (self.position, self.velocity);
            let mut acceleration = if (0.0..=max).contains(&x) {
                let magnitude = if v.abs() < h * FRICTION {
                    v.abs() / h
                } else {
                    FRICTION
                };
                if v >= 0.0 { -magnitude } else { magnitude }
            } else {
                let bound = if x < 0.0 { 0.0 } else { max };
                v * BAND_DAMPING + (bound - x) * BAND_STIFFNESS
            };
            if self.touching {
                acceleration +=
                    (follow - v) * FOLLOW_DAMPING + (self.target - x) * FOLLOW_STIFFNESS;
            }
            let v = v + h * acceleration;
            let x = x + h * v;
            let v = v.clamp(-MAX_VELOCITY, MAX_VELOCITY);
            (self.position, self.velocity) = if x < hard_low {
                (hard_low, v.max(0.0))
            } else if x > hard_high {
                (hard_high, v.min(0.0))
            } else {
                (x, v)
            };
            remaining -= SUBSTEP;
        }
        if !self.touching {
            self.target = self.position;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> ScrollMetrics {
        ScrollMetrics {
            content: 400.0,
            viewport: 100.0,
            gesture: true,
            allow_scroll_when_fits: true,
            ..ScrollMetrics::default()
        }
    }

    fn report(metrics: &ScrollMetrics) -> LayoutReport {
        LayoutReport {
            scrolls: [("v".to_owned(), metrics.clone())].into_iter().collect(),
            ..LayoutReport::default()
        }
    }

    // A dragged finger pans the content with it, then a release flings and settles in bounds.
    #[test]
    fn touch_pan_follows_the_finger_then_flings_and_settles() {
        let metrics = metrics();
        let report = report(&metrics);
        let mut state = ViewState::default();
        state.begin_scroll_touch("v", &metrics);
        for _ in 0..20 {
            state.scroll_touch_moved("v", &metrics, [0.0, -5.0]);
            state.step_scrolls(&report, 1.0 / 60.0);
        }
        let held = state.scroll_offset("v");
        assert!(held > 60.0 && held < 110.0, "follows the finger: {held}");
        assert!(!state.end_scroll_touch("v"), "a 100px drag is not a press");
        for _ in 0..600 {
            state.step_scrolls(&report, 1.0 / 60.0);
        }
        let rest = state.scroll_offset("v");
        assert!(rest > held && rest <= 300.0, "fling carries on: {rest}");
        assert!(state.scroll_state["v"].motion.is_none(), "settles");
    }

    // Past an end the rubber band pulls the offset back inside.
    #[test]
    fn overscroll_springs_back() {
        let metrics = metrics();
        let report = report(&metrics);
        let mut state = ViewState::default();
        state.begin_scroll_touch("v", &metrics);
        for _ in 0..10 {
            state.scroll_touch_moved("v", &metrics, [0.0, 10.0]);
            state.step_scrolls(&report, 1.0 / 60.0);
        }
        assert!(state.scroll_offset("v") < 0.0, "pulled past the top");
        assert!(
            state.scroll_offset("v") >= -25.0,
            "no further than a quarter viewport"
        );
        state.end_scroll_touch("v");
        for _ in 0..300 {
            state.step_scrolls(&report, 1.0 / 60.0);
        }
        assert!(state.scroll_offset("v") >= 0.0);
    }

    // Without gesture control, or with fitting content that disallows it, a drag does nothing.
    #[test]
    fn pan_needs_gesture_control_and_room() {
        let mut metrics = metrics();
        metrics.gesture = false;
        let mut state = ViewState::default();
        state.begin_scroll_touch("v", &metrics);
        state.scroll_touch_moved("v", &metrics, [0.0, -50.0]);
        state.step_scrolls(&report(&metrics), 0.1);
        assert_eq!(state.scroll_offset("v"), 0.0);
        let mut fits = self::metrics();
        fits.content = 50.0;
        fits.allow_scroll_when_fits = false;
        state.scroll_touch_moved("v", &fits, [0.0, -50.0]);
        assert!(state.end_scroll_touch("v"), "no travel stays a press");
    }

    // A touch-mode box shows on touch and fades out a second after release.
    #[test]
    fn touch_box_fades_after_release() {
        let mut metrics = metrics();
        metrics.touch_mode = true;
        let report = report(&metrics);
        let mut state = ViewState::default();
        state.begin_scroll_touch("v", &metrics);
        assert_eq!(state.scroll_state["v"].bar_fade, Some(1.0));
        state.step_scrolls(&report, 0.5);
        assert_eq!(
            state.scroll_state["v"].bar_fade,
            Some(1.0),
            "held stays shown"
        );
        state.end_scroll_touch("v");
        state.step_scrolls(&report, 0.5);
        state.step_scrolls(&report, 0.6);
        assert_eq!(state.scroll_state["v"].bar_fade, Some(0.0));
    }
}
