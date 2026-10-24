//! A finger captures its starting viewport so a swipe cannot activate a row or switch columns.

use super::MenuScrolls;
use ui::UiPoint;

pub(super) struct TouchScroll {
    key: String,
    start: UiPoint,
    last: UiPoint,
    consumed: bool,
    scrollbar: bool,
    thumb_drag: Option<(String, f32)>,
}

impl<F: PartialEq> MenuScrolls<F> {
    /// Captures a scrollable viewport or scrollbar, leaving a content tap pending until movement.
    pub fn begin_touch(&mut self, point: UiPoint) -> bool {
        self.touch = None;
        let Some(area) = self.at(point).filter(|area| area.max > 0.0) else {
            return false;
        };
        let key = area.key.clone();
        let scrollbar = self.press(point);
        self.touch = Some(TouchScroll {
            key,
            start: point,
            last: point,
            consumed: scrollbar,
            scrollbar,
            thumb_drag: self.drag.take(),
        });
        true
    }

    /// Follows the captured viewport; returns whether scrolling consumed this finger's press.
    /// A release includes its final position, while cancellation passes no position.
    pub fn move_touch(&mut self, point: Option<UiPoint>, held: bool) -> bool {
        let Some(mut touch) = self.touch.take() else {
            return false;
        };
        if !self.areas.iter().any(|area| area.key == touch.key) {
            self.drag = None;
            return true;
        }
        if let Some(point) = point {
            if touch.scrollbar {
                self.drag = touch.thumb_drag.take();
                self.drag(Some(point), true);
                touch.thumb_drag = self.drag.take();
            } else {
                touch.consumed |= (point.y() - touch.start.y()).abs() > ui::TOUCH_DRAG_DISTANCE;
                if touch.consumed {
                    self.scroll_by(&touch.key, point.y() - touch.last.y());
                }
                // Accumulate movement below the threshold, so the first drag applies it too.
                if touch.consumed {
                    touch.last = point;
                }
            }
        }
        let consumed = touch.consumed || point.is_none();
        if held {
            self.touch = Some(touch);
        } else {
            self.drag = None;
        }
        consumed
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{area, point};
    use super::*;

    /// Content follows the finger in its original column, at the published UI scale.
    #[test]
    fn swipe_captures_the_viewport_and_consumes_its_release() {
        let mut scrolls = MenuScrolls::<()>::default();
        scrolls.set_areas(vec![area()]);
        assert!(scrolls.begin_touch(point(50.0, 80.0)));
        assert!(!scrolls.move_touch(Some(point(50.0, 78.0)), true));
        assert!(scrolls.move_touch(Some(point(150.0, 20.0)), true));
        assert_eq!(scrolls.offsets()["list"], 30.0);
        assert!(scrolls.move_touch(Some(point(150.0, 0.0)), false));
        assert_eq!(scrolls.offsets()["list"], 40.0);
        assert!(!scrolls.move_touch(Some(point(50.0, 0.0)), true));
    }

    /// Small movements remain taps; cancellation and screen changes release capture.
    #[test]
    fn taps_cancellation_and_screen_changes_do_not_scroll() {
        let mut scrolls = MenuScrolls::<()>::default();
        scrolls.begin_frame("settings".into());
        scrolls.set_areas(vec![area()]);
        assert!(scrolls.begin_touch(point(50.0, 50.0)));
        assert!(!scrolls.move_touch(Some(point(50.0, 52.0)), false));
        assert!(scrolls.offsets().is_empty());
        assert!(scrolls.begin_touch(point(50.0, 50.0)));
        assert!(scrolls.move_touch(None, false));
        assert!(scrolls.begin_touch(point(50.0, 50.0)));
        scrolls.begin_frame("home".into());
        assert!(!scrolls.move_touch(Some(point(50.0, 0.0)), true));
    }

    /// Touching the thumb uses its grab offset rather than scrolling the content beneath it.
    #[test]
    fn touch_drags_the_scrollbar_thumb() {
        let mut scrolls = MenuScrolls::<()>::default();
        scrolls.set_areas(vec![area()]);
        assert!(scrolls.begin_touch(point(97.0, 10.0)));
        scrolls.drag(None, false);
        assert!(scrolls.move_touch(Some(point(97.0, 40.0)), true));
        scrolls.drag(None, false);
        assert!(scrolls.move_touch(Some(point(97.0, 85.0)), false));
        assert_eq!(scrolls.offsets()["list"], 150.0);
        assert!(!scrolls.dragging());
    }
}
