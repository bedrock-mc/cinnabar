//! Menu scroll input routed to the presentation's scroll views.

use ui::UiPoint;

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
