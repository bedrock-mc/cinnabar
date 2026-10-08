//! Menu scroll views: offsets kept across frames, the areas the last frame
//! drew (window-logical), and wheel, scrollbar-drag and track-press input.

use std::collections::HashMap;

use ui::{UiPoint, UiRect};

mod smoothing;
#[cfg(test)]
mod smoothing_tests;
use smoothing::ScrollTween;

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
    fn engine_at(&self, point: UiPoint) -> Option<(json_ui::ScrollMetrics, [f64; 2])> {
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
    fn offset_for_thumb(&self, y: f32) -> f32 {
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

#[derive(Default)]
pub struct MenuScrolls {
    offsets: HashMap<String, f32>,
    areas: Vec<ScrollArea>,
    /// The dragged view and the grab point's distance below its thumb's top
    /// (an engine view: the pointer's last virtual position along its axis).
    drag: Option<(String, f32)>,
    screen: Option<String>,
    focused: Option<crate::menu::MenuAction>,
    smooth: bool,
    seconds: f64,
    motion: HashMap<String, ScrollTween>,
}

impl MenuScrolls {
    /// Retention waits for wheel interpolation and captured scrollbar input.
    pub(super) fn active(&self) -> bool {
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
        }
    }

    /// Reveals a newly focused fallback control without overriding later wheel movement.
    pub fn reveal_focus(
        &mut self,
        key: &str,
        action: Option<crate::menu::MenuAction>,
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
        if self.focused != action {
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
        action: Option<crate::menu::MenuAction>,
        key: Option<&str>,
        frame: &crate::ui_runtime::forms::EngineFrame,
    ) {
        if self.focused == action {
            return;
        }
        self.focused = action;
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
    pub fn observe_focus(&mut self, action: Option<crate::menu::MenuAction>) {
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

    pub(super) fn clear_areas(&mut self) {
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
    pub(super) fn scroll_by(&mut self, key: &str, pixels: f32) {
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

impl super::UiPresentationRuntime {
    /// Scrolls the menu view under `point`; `true` when one took the wheel.
    pub fn scroll_menu(&mut self, point: UiPoint, notches: f32, pixels: bool) -> bool {
        self.menu_scrolls.wheel(point, notches, pixels)
    }

    /// A menu press on a scrollbar, which then takes no button action.
    pub fn press_menu_scrollbar(&mut self, point: UiPoint) -> bool {
        self.menu_scrolls.press(point)
    }

    /// Follows a held scrollbar drag; `true` while one is live.
    pub fn drag_menu_scroll(&mut self, point: Option<UiPoint>, held: bool) -> bool {
        self.menu_scrolls.drag(point, held);
        self.menu_scrolls.dragging()
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
        let mut scrolls = MenuScrolls::default();
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
