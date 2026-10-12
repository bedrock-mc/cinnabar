//! Menu scroll views: offsets kept across frames, the areas the last frame
//! drew (window-logical), and wheel, scrollbar-drag and track-press input.

use std::collections::HashMap;

use crate::scroll::{ScrollArea, ScrollTween};
use ui::{UiPoint, UiRect};

#[cfg(test)]
mod smoothing_tests;

/// Scroll state for one menu; `F` names the focused control a reveal follows.
pub struct MenuScrolls<F> {
    offsets: HashMap<String, f32>,
    areas: Vec<ScrollArea>,
    /// The dragged view and the grab point's distance below its thumb's top
    /// (an engine view: the pointer's last virtual position along its axis).
    drag: Option<(String, f32)>,
    screen: Option<String>,
    focused: Option<F>,
    focused_bounds: Option<UiRect>,
    smooth: bool,
    seconds: f64,
    motion: HashMap<String, ScrollTween>,
}

impl<F> Default for MenuScrolls<F> {
    fn default() -> Self {
        Self {
            offsets: HashMap::new(),
            areas: Vec::new(),
            drag: None,
            screen: None,
            focused: None,
            focused_bounds: None,
            smooth: false,
            seconds: 0.0,
            motion: HashMap::new(),
        }
    }
}

impl<F: PartialEq> MenuScrolls<F> {
    /// Retention waits for wheel interpolation and captured scrollbar input.
    pub fn active(&self) -> bool {
        self.drag.is_some() || !self.motion.is_empty()
    }

    /// Samples wheel motion once per rendered menu frame using the real menu clock.
    pub fn configure_motion(&mut self, smooth: bool, seconds: f64) {
        self.smooth = smooth;
        self.seconds = seconds;
        self.motion.retain(|key, motion| {
            let value = if smooth {
                motion.sample(seconds)
            } else {
                motion.target
            };
            if let Some(offset) = self.offsets.get_mut(key) {
                *offset = value;
            }
            if let Some(area) = self.areas.iter_mut().find(|area| &area.key == key) {
                area.offset = value.clamp(0.0, area.max);
            }
            smooth && !motion.settled(seconds)
        });
    }

    /// Forget every offset when the menu shows another screen.
    pub fn begin_frame(&mut self, screen: String) {
        if self.screen.as_ref() != Some(&screen) {
            self.offsets.clear();
            self.motion.clear();
            self.drag = None;
            self.screen = Some(screen);
            self.focused = None;
            self.focused_bounds = None;
        }
    }

    /// Reveals newly focused or growing controls without overriding later wheel movement.
    pub fn reveal_focus(
        &mut self,
        key: &str,
        action: Option<F>,
        bounds: Option<UiRect>,
        viewport: UiRect,
        max: f32,
    ) -> f32 {
        let mut offset = self
            .offsets
            .get(key)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, max);
        let grew = bounds
            .zip(self.focused_bounds)
            .is_some_and(|(current, previous)| current.height() > previous.height() + 0.5);
        self.focused_bounds = bounds;
        if self.focused != action || grew {
            self.focused = action;
            if let Some(bounds) = bounds {
                let top = bounds.min().y() - offset;
                let bottom = bounds.max().y() - offset;
                if top < viewport.min().y() {
                    offset -= viewport.min().y() - top;
                } else if bottom > viewport.max().y() {
                    offset += bottom - viewport.max().y();
                }
                offset = offset.clamp(0.0, max);
                self.offsets.insert(key.to_owned(), offset);
                self.motion.remove(key);
            }
        }
        offset
    }

    /// Reveals newly focused JSON-UI content without undoing later pointer scrolling.
    pub fn reveal_engine_focus(
        &mut self,
        action: Option<F>,
        key: Option<&str>,
        frame: &crate::frame::EngineFrame,
    ) {
        if self.focused == action {
            return;
        }
        self.focused = action;
        self.focused_bounds = None;
        let Some(region) = frame
            .hits
            .iter()
            .find(|region| Some(region.key.as_str()) == key)
        else {
            return;
        };
        let Some((key, metrics)) = frame
            .report
            .scrolls
            .iter()
            .filter(|(key, _)| {
                region
                    .key
                    .strip_prefix(key.as_str())
                    .is_some_and(|suffix| suffix.starts_with('/'))
            })
            .max_by_key(|(key, _)| key.len())
        else {
            return;
        };
        let (start, length) = if metrics.horizontal {
            (region.rect.x, region.rect.w)
        } else {
            (region.rect.y, region.rect.h)
        };
        self.offsets.insert(
            key.clone(),
            metrics.offset_revealing(start, start + length) as f32,
        );
    }

    pub fn offsets(&self) -> &HashMap<String, f32> {
        &self.offsets
    }

    /// Records focus outside scroll views so returning to a view can reveal its control.
    pub fn observe_focus(&mut self, action: Option<F>) {
        if self.focused != action {
            self.focused_bounds = None;
        }
        self.focused = action;
    }

    pub fn set_areas(&mut self, areas: Vec<ScrollArea>) {
        self.areas = areas;
        self.motion.retain(|key, motion| {
            let Some(area) = self.areas.iter().find(|area| &area.key == key) else {
                return false;
            };
            motion.clamp(area.max);
            true
        });
        for area in &self.areas {
            if let Some(offset) = self.offsets.get_mut(&area.key) {
                *offset = offset.clamp(0.0, area.max);
            }
        }
    }

    pub fn clear_areas(&mut self) {
        self.areas.clear();
    }

    fn at(&self, point: UiPoint) -> Option<&ScrollArea> {
        self.areas
            .iter()
            .rev()
            .find(|area| area.viewport.contains(point))
    }

    fn set(&mut self, key: &str, offset: f32) {
        self.motion.remove(key);
        if let Some(area) = self.areas.iter_mut().find(|area| area.key == key) {
            area.offset = offset.clamp(0.0, area.max);
            self.offsets.insert(key.to_owned(), area.offset);
        }
    }

    /// Direct edge scrolling during a captured drag follows the pointer without wheel easing.
    pub fn scroll_by(&mut self, key: &str, pixels: f32) {
        if let Some(area) = self.areas.iter().find(|area| area.key == key) {
            self.set(key, area.offset - pixels / area.scale);
        }
    }

    /// Scrolls the view under `point` by `notches` (lines) or window pixels.
    pub fn wheel(&mut self, point: UiPoint, notches: f32, pixels: bool) -> bool {
        if !notches.is_finite() {
            return false;
        }
        let Some(area) = self.at(point) else {
            return false;
        };
        let base = self
            .motion
            .get(&area.key)
            .map_or(area.offset, |motion| motion.target);
        let offset = match area.engine_at(point) {
            Some((metrics, _)) if !pixels => metrics.offset_for_wheel(f64::from(notches)) as f32,
            _ if pixels => base - notches / area.scale,
            _ => base - notches * area.speed,
        };
        let key = area.key.clone();
        let target = offset.clamp(0.0, area.max);
        if self.smooth && area.engine.is_none() {
            let current = area.offset;
            self.offsets.entry(key.clone()).or_insert(current);
            self.motion
                .entry(key)
                .and_modify(|motion| motion.retarget(target, pixels, self.seconds))
                .or_insert_with(|| ScrollTween::new(current, target, pixels, self.seconds));
        } else {
            self.set(&key, target);
        }
        true
    }

    /// A press on a scrollbar: grabs a draggable thumb, or centres the view on
    /// the track fraction pressed. `true` when the press belonged to a scrollbar.
    pub fn press(&mut self, point: UiPoint) -> bool {
        let Some(area) = self
            .areas
            .iter()
            .rev()
            .find(|area| area.track.is_some_and(|track| track.contains(point)))
        else {
            return false;
        };
        let key = area.key.clone();
        self.motion.remove(&key);
        if let Some((metrics, at)) = area.engine_at(point) {
            let along = at[usize::from(!metrics.horizontal)] as f32;
            match area.thumb {
                Some(thumb) if thumb.contains(point) => {
                    if area.draggable {
                        self.drag = Some((key, along));
                    }
                }
                // A track press jumps only when the view names its track button.
                _ if metrics.track_button.is_some() => {
                    self.set(&key, metrics.offset_for_track(at) as f32);
                }
                _ => {}
            }
            return true;
        }
        match (area.thumb, area.track) {
            (Some(thumb), _) if thumb.contains(point) => {
                if area.draggable {
                    self.drag = Some((key, point.y() - thumb.min().y()));
                }
            }
            (_, Some(track)) => {
                let view = area.viewport.height() / area.scale;
                let fraction = if track.height() > 0.5 * area.scale {
                    (point.y() - track.min().y()) / track.height()
                } else {
                    1.0
                };
                self.set(&key, view * -0.5 + fraction * (area.max + view));
            }
            _ => {}
        }
        true
    }

    /// Follows a held thumb drag; a release ends it.
    pub fn drag(&mut self, point: Option<UiPoint>, held: bool) {
        if !held {
            self.drag = None;
            return;
        }
        let (Some((key, grab)), Some(point)) = (self.drag.clone(), point) else {
            return;
        };
        let Some(area) = self.areas.iter().find(|area| area.key == key) else {
            return;
        };
        if let Some((metrics, at)) = area.engine_at(point) {
            let along = at[usize::from(!metrics.horizontal)];
            let offset = metrics.thumb_drag_target(along - f64::from(grab)) as f32;
            self.set(&key, offset);
            self.drag = Some((key, along as f32));
            return;
        }
        let offset = area.offset_for_thumb(point.y() - grab);
        self.set(&key, offset);
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn area() -> ScrollArea {
        let rect = |x0, y0, x1, y1| UiRect::new(point(x0, y0), point(x1, y1)).unwrap();
        ScrollArea {
            key: "list".to_owned(),
            viewport: rect(0.0, 0.0, 100.0, 100.0),
            scale: 2.0,
            offset: 0.0,
            max: 150.0,
            speed: 10.0,
            track: Some(rect(95.0, 0.0, 100.0, 100.0)),
            thumb: Some(rect(95.0, 0.0, 100.0, 25.0)),
            engine: None,
            draggable: true,
        }
    }

    pub(super) fn point(x: f32, y: f32) -> UiPoint {
        UiPoint::new(x, y).unwrap()
    }

    // Wheel, track press and thumb drag each move the view under the pointer,
    // clamped to its content; a track press centres on its fraction.
    #[test]
    fn wheel_track_and_thumb_scroll_the_view() {
        let mut scrolls = MenuScrolls::<()>::default();
        scrolls.set_areas(vec![area()]);
        assert!(scrolls.wheel(point(50.0, 50.0), -2.0, false));
        assert_eq!(scrolls.offsets()["list"], 20.0);
        assert!(!scrolls.wheel(point(150.0, 50.0), -2.0, false));
        assert!(scrolls.press(point(97.0, 80.0)));
        assert_eq!(scrolls.offsets()["list"], 135.0);
        scrolls.set_areas(vec![area()]);
        assert!(scrolls.press(point(97.0, 10.0)));
        scrolls.drag(Some(point(97.0, 85.0)), true);
        assert_eq!(scrolls.offsets()["list"], 150.0);
        scrolls.drag(None, false);
        assert!(!scrolls.dragging());
        scrolls.begin_frame("other".to_owned());
        assert!(scrolls.offsets().is_empty());
    }
}
