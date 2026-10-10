//! Wheel and scrollbar input for extension-owned personal-panel scroll views.

use json_ui::{Draggable, HitKind, ScrollMetrics};

use super::*;

impl UiPresentationRuntime {
    /// Scrolls the personal panel under a window-logical pointer. Positive wheel deltas
    /// move up; pixel deltas are converted through the panel's actual presentation scale.
    pub fn scroll_mod_panel(&mut self, position: [f32; 2], delta: f64, pixels: bool) -> bool {
        let Some(panel) = self
            .form_presentation
            .mod_panel
            .as_mut()
            .filter(|panel| panel.open)
        else {
            return false;
        };
        if !delta.is_finite() || delta == 0.0 || !position.iter().all(|value| value.is_finite()) {
            return false;
        }
        let Some(frame) = panel.frame.as_ref() else {
            return false;
        };
        let point = [
            f64::from((position[0] - frame.origin[0]) / frame.scale),
            f64::from((position[1] - frame.origin[1]) / frame.scale),
        ];
        let Some(region) = json_ui::scroll_target(&frame.hits, &frame.report, point) else {
            return false;
        };
        let Some(metrics) = frame.report.scrolls.get(&region.key) else {
            return false;
        };
        let current = ScrollMetrics {
            offset: panel
                .view
                .scroll
                .get(&region.key)
                .copied()
                .unwrap_or(metrics.offset),
            ..metrics.clone()
        };
        let offset = if pixels {
            (current.offset - delta / f64::from(frame.scale)).clamp(0.0, current.max_offset())
        } else {
            current.offset_for_wheel(delta)
        };
        panel.view.scroll.insert(region.key.clone(), offset);
        true
    }
}

impl ModPanel {
    /// Consumes track presses and box drags, including the final release position.
    pub(super) fn scroll_pointer(&mut self, point: [f64; 2], pressed: bool, held: bool) -> bool {
        let Some(frame) = self.frame.as_ref() else {
            return false;
        };
        if let Some((key, last)) = self.scroll_drag.clone() {
            if let Some(metrics) = frame.report.scrolls.get(&key) {
                let at = along(metrics, point);
                let current = ScrollMetrics {
                    offset: self
                        .view
                        .scroll
                        .get(&key)
                        .copied()
                        .unwrap_or(metrics.offset),
                    ..metrics.clone()
                };
                self.view
                    .scroll
                    .insert(key.clone(), current.thumb_drag_target(at - last));
                self.scroll_drag = held.then_some((key, at));
            } else {
                self.scroll_drag = None;
            }
            return true;
        }
        if !pressed || self.edit.is_some() {
            return false;
        }
        let Some(region) = frame.hits.iter().rev().find(|region| {
            region.enabled
                && region.contains(point)
                && (region.pressed.is_some()
                    || matches!(region.kind, HitKind::ScrollBox | HitKind::ScrollTrack))
        }) else {
            return false;
        };
        if !matches!(region.kind, HitKind::ScrollBox | HitKind::ScrollTrack) {
            return false;
        }
        let Some(view) = frame
            .hits
            .iter()
            .filter(|view| view.kind == HitKind::ScrollView && region.key.starts_with(&view.key))
            .max_by_key(|view| view.key.len())
        else {
            return false;
        };
        let Some(metrics) = frame.report.scrolls.get(&view.key) else {
            return false;
        };
        if region.kind == HitKind::ScrollBox && metrics.box_drag != Draggable::NotDraggable {
            self.scroll_drag = Some((view.key.clone(), along(metrics, point)));
            return true;
        }
        if region.kind == HitKind::ScrollTrack
            && region.pressed.is_some()
            && region.pressed == metrics.track_button
        {
            self.view
                .scroll
                .insert(view.key.clone(), metrics.offset_for_track(point));
            return true;
        }
        false
    }
}

/// Reads the pointer along the scroll view's declared axis.
fn along(metrics: &ScrollMetrics, point: [f64; 2]) -> f64 {
    point[usize::from(!metrics.horizontal)]
}
