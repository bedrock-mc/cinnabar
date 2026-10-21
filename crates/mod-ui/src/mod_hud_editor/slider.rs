//! Horizontal slider gestures on painted surface regions: selected-card scale and guest sliders.

use super::*;
use ui::mod_hud::{EDITOR_SCALE_SLIDER_RANGE, MAX_CARD_SCALE, MIN_CARD_SCALE};
use ui::mod_panel::{Control, Event};

const SCALE_STEP: f64 = 0.05;
/// Float noise below this is the same scale, so pressing at the current value changes nothing.
const SCALE_EPSILON: f32 = 1e-4;

/// One captured slider, located by the key of the hit region that was pressed.
pub(super) struct Gesture {
    key: String,
    target: Target,
}

enum Target {
    /// Card by ID, so republished reordering keeps the gesture and its pre-gesture scale.
    Scale { id: String, before: f32 },
    /// Guest `Control::Slider` by panel index.
    Control(usize),
}

/// Converts a window-logical pointer position into the frame's GUI pixels.
pub(super) fn frame_point(frame: &EngineFrame, position: [f32; 2]) -> [f64; 2] {
    std::array::from_fn(|axis| f64::from((position[axis] - frame.origin[axis]) / frame.scale))
}

/// Fill fraction of the scale slider for `scale`, clamped to `0..=1`.
pub(super) fn scale_fill(scale: f32) -> f64 {
    let [lo, hi] = EDITOR_SCALE_SLIDER_RANGE.map(f64::from);
    ((f64::from(scale) - lo) / (hi - lo)).clamp(0., 1.)
}

impl HudEditor {
    /// Captures `hud.scale` or a slider `hud.control:N` press at `point` and applies it; `None`
    /// leaves every other action to the caller. `hud.scale` without a visible selection is a no-op.
    pub(super) fn begin_slider(
        &mut self,
        action: &str,
        point: [f64; 2],
        controls: &mut [Control],
    ) -> Option<Vec<Event>> {
        let target = if action == "hud.scale" {
            self.cancel_drag();
            let Some(card) = self
                .selected
                .and_then(|index| self.draft.cards.get(index))
                .filter(|card| !self.hidden.contains(&card.id))
            else {
                return Some(Vec::new());
            };
            let target = Target::Scale {
                id: card.id.clone(),
                before: card.scale,
            };
            self.settle_anchor_motion();
            target
        } else {
            let index = action.strip_prefix("hud.control:")?.parse::<usize>().ok()?;
            if !matches!(controls.get(index), Some(Control::Slider { .. })) {
                return None;
            }
            self.cancel_drag();
            Target::Control(index)
        };
        let key = self.view.pressed.clone()?;
        self.slider = Some(Gesture { key, target });
        Some(self.slide(point, controls))
    }

    /// Continues a captured slider until its physical release, bypassing hover and card capture.
    pub(super) fn drag_slider(
        &mut self,
        position: [f32; 2],
        held: bool,
        controls: &mut [Control],
    ) -> Vec<Event> {
        let point = self
            .frame
            .as_ref()
            .map(|frame| frame_point(frame, position));
        let events = point.map_or_else(Vec::new, |point| self.slide(point, controls));
        if !held {
            self.view.pressed = None;
            self.data_dirty = true;
            self.end_slider();
        }
        events
    }

    /// Maps the pointer's fraction across the captured region onto its target; a guest slider
    /// emits one event per stepped change and a scale change invalidates only the catalog.
    fn slide(&mut self, point: [f64; 2], controls: &mut [Control]) -> Vec<Event> {
        let Some(gesture) = &self.slider else {
            return Vec::new();
        };
        let Some(fraction) = self.frame.as_ref().and_then(|frame| {
            let region = frame.hits.iter().find(|hit| hit.key == gesture.key)?;
            Some(region.fraction_at(region.local(point)[0]))
        }) else {
            return Vec::new();
        };
        match gesture.target {
            Target::Scale { ref id, .. } => {
                let Some(card) = self.draft.cards.iter_mut().find(|card| &card.id == id) else {
                    return Vec::new();
                };
                let [lo, hi] = EDITOR_SCALE_SLIDER_RANGE.map(f64::from);
                let scale =
                    (((lo + fraction * (hi - lo)) / SCALE_STEP).round() * SCALE_STEP) as f32;
                let scale = scale.clamp(MIN_CARD_SCALE, MAX_CARD_SCALE);
                if (card.scale - scale).abs() > SCALE_EPSILON {
                    card.scale = scale;
                    self.catalog = None;
                    self.data_dirty = true;
                }
                Vec::new()
            }
            Target::Control(index) => {
                let Some(Control::Slider {
                    id,
                    value,
                    min,
                    max,
                    step,
                    ..
                }) = controls.get_mut(index)
                else {
                    return Vec::new();
                };
                let raw = *min + fraction as f32 * (*max - *min);
                let next = (*min + ((raw - *min) / *step).round() * *step).clamp(*min, *max);
                if *value == next {
                    return Vec::new();
                }
                *value = next;
                vec![Event {
                    id: id.clone(),
                    value: next,
                }]
            }
        }
    }

    /// Whether a held scale gesture keeps the inspector, and so its slider, in place.
    pub(super) fn scaling(&self) -> bool {
        matches!(
            self.slider,
            Some(Gesture {
                target: Target::Scale { .. },
                ..
            })
        )
    }

    /// Releases a slider, recording one history step when it left the card's scale changed.
    fn end_slider(&mut self) {
        self.data_dirty = true;
        if let Some(Gesture {
            target: Target::Scale { id, before },
            ..
        }) = self.slider.take()
            && self
                .draft
                .cards
                .iter()
                .any(|card| card.id == id && card.scale != before)
        {
            self.checkpoint();
        }
    }

    /// Drops an unfinished slider, restoring the card's pre-gesture scale.
    pub(super) fn cancel_slider(&mut self) {
        if let Some(Gesture {
            target: Target::Scale { id, before },
            ..
        }) = self.slider.take()
            && let Some(card) = self.draft.cards.iter_mut().find(|card| card.id == id)
            && card.scale != before
        {
            card.scale = before;
            self.catalog = None;
            self.data_dirty = true;
        }
    }

    /// Mirrors every guest slider's fill; returns whether any fill changed.
    pub(super) fn sync_control_fills(&mut self, controls: &[Control]) -> bool {
        let fills = controls
            .iter()
            .enumerate()
            .filter_map(|(index, control)| match control {
                Control::Slider {
                    value, min, max, ..
                } => Some((index, f64::from((value - min) / (max - min)).clamp(0., 1.))),
                _ => None,
            });
        if fills.clone().eq(self.control_fills.iter().copied()) {
            return false;
        }
        self.control_fills.clear();
        self.control_fills.extend(fills);
        true
    }
}
