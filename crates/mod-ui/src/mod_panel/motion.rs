//! Bounded, opt-in panel feedback and wheel easing driven by the caller's clock.

use std::collections::HashMap;

use json_ui::{DataSource, Scalar, ViewState};
use serde_json::json;
use ui::mod_panel::{Control, Panel, Theme};

use super::{transitions::SurfaceTransitions, widgets::Palette};

const FEEDBACK_SECONDS: f64 = 0.10;
const SCROLL_SECONDS: f64 = 0.14;

#[derive(Clone, Copy)]
struct Tween {
    from: f64,
    to: f64,
    began: f64,
    duration: f64,
}

impl Tween {
    /// Keeps unchanged values exact and eases a retarget from its current displayed value.
    fn retarget(&mut self, target: f64, now: f64, duration: f64) -> bool {
        if self.to == target {
            return false;
        }
        self.from = self.sample(now);
        self.to = target;
        self.began = now;
        self.duration = duration;
        true
    }

    /// An out-cubic sample with a finite end, without frame-rate-dependent integration; a zero
    /// duration shows the target at once.
    fn sample(self, now: f64) -> f64 {
        let t = if self.duration > 0.0 {
            ((now - self.began) / self.duration).clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.from + (self.to - self.from) * (1.0 - (1.0 - t).powi(3))
    }

    /// Jumps to the target, reporting whether the value displayed at `now` changed.
    fn settle(&mut self, now: f64) -> bool {
        let moved = self.sample(now) != self.to;
        self.from = self.to;
        moved
    }

    fn active(self, now: f64) -> bool {
        self.from != self.to && now < self.began + self.duration
    }

    fn still(value: f64) -> Self {
        Self {
            from: value,
            to: value,
            began: 0.0,
            duration: FEEDBACK_SECONDS,
        }
    }
}

#[derive(Default)]
pub(super) struct PanelMotion {
    pub(super) now: f64,
    toggles: HashMap<usize, Tween>,
    hover: HashMap<usize, Tween>,
    scrolls: HashMap<String, Tween>,
    /// Authored surface bindings easing toward their latest values.
    pub(super) transitions: SurfaceTransitions,
}

impl PanelMotion {
    /// Reports whether clearing motion would leave an intermediate toggle, hover highlight or
    /// transitioned binding.
    pub(super) fn needs_feedback_reset(&self) -> bool {
        self.transitions.active()
            || self
                .toggles
                .values()
                .any(|tween| tween.sample(self.now) != tween.to)
            || self
                .hover
                .values()
                .any(|tween| tween.sample(self.now) != 0.0)
    }

    /// Reads the outstanding wheel destination, keeping rapid reversals lossless.
    pub(super) fn scroll_target(&self, key: &str, current: f64) -> f64 {
        self.scrolls.get(key).map_or(current, |tween| tween.to)
    }

    /// Drops out-of-range motion when a filter or resize reduces the content extent.
    pub(super) fn scroll_extent(&mut self, key: &str, max: f64) {
        if let Some(tween) = self.scrolls.get_mut(key) {
            tween.from = tween.from.clamp(0.0, max);
            tween.to = tween.to.clamp(0.0, max);
        }
    }

    /// Retargets wheel motion from the last rendered offset, accumulating pending notches.
    pub(super) fn scroll(&mut self, key: &str, current: f64, delta: f64, max: f64) {
        let tween = self
            .scrolls
            .entry(key.to_owned())
            .or_insert_with(|| Tween::still(current));
        tween.retarget((tween.to + delta).clamp(0.0, max), self.now, SCROLL_SECONDS);
    }

    /// Pointer drags and pixel scrolling override any outstanding wheel target immediately.
    pub(super) fn cancel_scroll(&mut self, key: &str) {
        self.scrolls.remove(key);
    }

    /// Samples all current feedback once and reports whether visible values moved. Without
    /// `animate`, feedback and pending wheel motion reach their targets at once.
    pub(super) fn advance(
        &mut self,
        panel: &Panel,
        view: &mut ViewState,
        hovered: Option<usize>,
        now: f64,
        animate: bool,
    ) -> bool {
        if !now.is_finite() {
            return false;
        }
        let previous = self.now;
        self.now = now.max(previous);
        let now = self.now;
        let mut changed = false;
        let feedback = if animate { FEEDBACK_SECONDS } else { 0.0 };
        if !animate {
            for tween in self
                .toggles
                .values_mut()
                .chain(self.hover.values_mut())
                .chain(self.scrolls.values_mut())
            {
                changed |= tween.settle(now);
            }
        }
        for (index, control) in panel.controls.iter().enumerate() {
            if let Control::Toggle { value, .. } = control {
                let value = f64::from(u8::from(*value));
                let tween = self
                    .toggles
                    .entry(index)
                    .or_insert_with(|| Tween::still(value));
                changed |= tween.retarget(value, now, feedback);
                changed |= tween.sample(previous) != tween.sample(now);
            }
        }
        if let Some(index) = hovered {
            self.hover.entry(index).or_insert_with(|| Tween::still(0.0));
        }
        for (index, tween) in &mut self.hover {
            changed |= tween.retarget(f64::from(u8::from(hovered == Some(*index))), now, feedback);
            changed |= tween.sample(previous) != tween.sample(now);
        }
        self.scrolls.retain(|key, tween| {
            let offset = tween.sample(now);
            changed |= view.scroll.get(key).copied().unwrap_or(0.0) != offset;
            view.scroll.insert(key.clone(), offset);
            tween.active(now)
        });
        changed
    }

    /// Publishes generic native values and transitioned bindings without changing the retained
    /// document or schema.
    pub(super) fn write(&self, panel: &Panel, data: &mut DataSource) {
        self.transitions.write(data);
        let palette = Palette::for_panel(panel);
        let mix = |from: [f64; 4], to: [f64; 4], progress: f64| {
            std::array::from_fn::<_, 4, _>(|index| {
                from[index] + (to[index] - from[index]) * progress
            })
        };
        for (index, tween) in &self.toggles {
            let progress = tween.sample(self.now);
            data.set_global(
                format!("#row_{index}_toggle_progress"),
                Scalar::Num(progress),
            );
            data.set_global(
                format!("#row_{index}_toggle_inverse_progress"),
                Scalar::Num(1.0 - progress),
            );
            data.set_global(
                format!("#row_{index}_knob_offset"),
                Scalar::Json(json!([2.0 + 10.0 * progress, 2.0])),
            );
            data.set_global(
                format!("#row_{index}_toggle_color"),
                Scalar::Json(json!(mix(palette.raised, palette.accent, progress))),
            );
            let color = if panel.theme == Theme::Monochrome {
                mix(palette.text, palette.background, progress)
            } else {
                [1.0; 4]
            };
            data.set_global(
                format!("#row_{index}_knob_color"),
                Scalar::Json(json!(color)),
            );
        }
        for (index, tween) in &self.hover {
            data.set_global(
                format!("#row_{index}_hover"),
                Scalar::Num(tween.sample(self.now)),
            );
        }
    }

    /// Lets an idle renderer keep requesting frames only until feedback has settled.
    pub(super) fn active(&self) -> bool {
        self.toggles
            .values()
            .chain(self.hover.values())
            .chain(self.scrolls.values())
            .any(|tween| tween.active(self.now))
            || self.transitions.active()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Multiple wheel notches accumulate and pixel/drag overrides remain immediate.
    #[test]
    fn wheel_targets_accumulate_and_settle_at_the_exact_offset() {
        let mut motion = PanelMotion::default();
        let mut view = ViewState::default();
        let panel = crate::test_support::sample_panel();
        motion.scroll("list", 0.0, 30.0, 100.0);
        motion.scroll("list", 0.0, 30.0, 100.0);
        motion.advance(&panel, &mut view, None, 0.03, true);
        let middle = view.scroll["list"];
        assert!(middle > 0.0 && middle < 60.0);
        motion.scroll("list", middle, -10.0, 100.0);
        motion.advance(&panel, &mut view, None, 0.5, true);
        assert_eq!(view.scroll["list"], 50.0);
        assert!(!motion.active());
        motion.scroll("list", 50.0, 30.0, 100.0);
        motion.cancel_scroll("list");
        assert!(!motion.active());
    }

    /// Feedback samples the caller clock and preserves its current value when reversed.
    #[test]
    fn native_toggle_and_hover_feedback_ease_and_return_to_idle() {
        let mut motion = PanelMotion::default();
        let mut view = ViewState::default();
        let mut panel = crate::test_support::sample_panel();
        motion.advance(&panel, &mut view, None, 1.0, true);
        if let Control::Toggle { value, .. } = &mut panel.controls[0] {
            *value = true;
        }
        motion.advance(&panel, &mut view, Some(0), 1.01, true);
        assert_eq!(motion.toggles[&0].sample(motion.now), 0.0);
        motion.advance(&panel, &mut view, Some(0), 1.04, true);
        let middle = motion.toggles[&0].sample(motion.now);
        assert!(middle > 0.0 && middle < 1.0);
        if let Control::Toggle { value, .. } = &mut panel.controls[0] {
            *value = false;
        }
        motion.advance(&panel, &mut view, None, 1.04, true);
        assert_eq!(motion.toggles[&0].sample(motion.now), middle);
        motion.advance(&panel, &mut view, None, 1.4, true);
        assert_eq!(motion.toggles[&0].sample(motion.now), 0.0);
        assert_eq!(motion.hover[&0].sample(motion.now), 0.0);
        assert!(!motion.active());
    }

    /// With animations off, toggles, hover and wheel targets apply at once, and feedback
    /// already moving jumps to its target.
    #[test]
    fn disabled_animation_applies_feedback_and_wheel_targets_at_once() {
        let mut motion = PanelMotion::default();
        let mut view = ViewState::default();
        let mut panel = crate::test_support::sample_panel();
        motion.advance(&panel, &mut view, None, 1.0, true);
        if let Control::Toggle { value, .. } = &mut panel.controls[0] {
            *value = true;
        }
        motion.advance(&panel, &mut view, Some(0), 1.01, true);
        motion.advance(&panel, &mut view, Some(0), 1.03, true);
        assert!(motion.active());
        assert!(motion.advance(&panel, &mut view, Some(0), 1.04, false));
        assert_eq!(motion.toggles[&0].sample(motion.now), 1.0);
        assert_eq!(motion.hover[&0].sample(motion.now), 1.0);
        assert!(!motion.active());

        if let Control::Toggle { value, .. } = &mut panel.controls[0] {
            *value = false;
        }
        assert!(motion.advance(&panel, &mut view, None, 1.05, false));
        assert_eq!(motion.toggles[&0].sample(motion.now), 0.0);
        assert_eq!(motion.hover[&0].sample(motion.now), 0.0);
        motion.scroll("list", 0.0, 30.0, 100.0);
        assert!(motion.advance(&panel, &mut view, None, 1.05, false));
        assert_eq!(view.scroll["list"], 30.0);
        assert!(!motion.active());
    }
}
