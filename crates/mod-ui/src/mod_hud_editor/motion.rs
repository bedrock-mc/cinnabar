//! Caller-clock presentation for anchor changes and the hide toast.

use super::*;

const ANCHOR_SECONDS: f64 = 0.3;
const TOAST_SECONDS: f64 = 4.;

#[derive(Default)]
pub(super) struct Toast {
    pub(super) text: String,
    started: Option<f64>,
}

impl Toast {
    /// Starts a hide message at the next caller-provided frame time.
    pub(super) fn new(text: String) -> Self {
        Self {
            text,
            started: None,
        }
    }
}

pub(super) struct AnchorMotion {
    index: usize,
    from: [f64; 2],
    at: [f64; 2],
    started: Option<f64>,
}

impl AnchorMotion {
    /// Retains the currently displayed origin until the first animation sample arrives.
    pub(super) fn new(index: usize, from: [f64; 2]) -> Self {
        Self {
            index,
            from,
            at: from,
            started: None,
        }
    }
}

impl HudEditor {
    /// Settles an interrupted anchor transition and refreshes its final bound position.
    pub(super) fn settle_anchor_motion(&mut self) {
        self.data_dirty |= self.anchor_motion.take().is_some();
    }

    /// Reads the same displayed origin for artwork, bounds and inspector bindings.
    pub(super) fn card_origin(&self, index: usize) -> [f64; 2] {
        self.anchor_motion
            .as_ref()
            .filter(|motion| motion.index == index)
            .map_or_else(
                || cards::origin(&self.draft.cards[index], self.viewport),
                |motion| motion.at,
            )
    }

    /// Advances transient visuals, leaving a steady-state frame clean and allocation-free.
    pub(super) fn advance_feedback(&mut self, now: f64, animate: bool) {
        if !now.is_finite() {
            return;
        }
        if !self.toast.text.is_empty() {
            let started = *self.toast.started.get_or_insert(now);
            if now - started >= TOAST_SECONDS {
                self.toast = Default::default();
                self.data_dirty = true;
            }
        }
        if let Some(motion) = &mut self.anchor_motion {
            let started = *motion.started.get_or_insert(now);
            let progress = if animate {
                ((now - started) / ANCHOR_SECONDS).clamp(0., 1.)
            } else {
                1.
            };
            let target = cards::origin(&self.draft.cards[motion.index], self.viewport);
            let eased = 1. - (1. - progress).powi(3);
            let at = std::array::from_fn(|axis| {
                motion.from[axis] + (target[axis] - motion.from[axis]) * eased
            });
            self.data_dirty |= motion.at != at;
            motion.at = at;
            if progress >= 1. {
                self.anchor_motion = None;
            }
        }
    }
}
