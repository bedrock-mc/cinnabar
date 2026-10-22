//! Settings-owned pointer capture prevents editing from activating gameplay or other menus.
use super::*;
use std::sync::Arc;

/// Delivers only one owning finger; its cancellation rolls back the unfinished drag.
pub(super) fn drive(
    menu: &mut MenuRuntime,
    presentation: &mut UiPresentationRuntime,
    touches: &Touches,
    capture: &mut GuiScaleDrag,
    pointer: Option<[f32; 2]>,
    buttons: &ButtonInput<MouseButton>,
) {
    if capture.editor_touch.is_none()
        && !capture.left_held
        && let Some(touch) = touches.iter_just_pressed().next()
    {
        capture.editor_touch = Some(Capture::begin(
            presentation,
            touch.id(),
            touch.position().to_array(),
        ));
    }
    if let Some(owner) = capture.editor_touch.as_mut() {
        if !owner.advance(presentation, touches) {
            capture.editor_touch = None;
        }
    } else if let Some(at) = pointer {
        presentation.touch_editor_pointer(
            at,
            buttons.just_pressed(MouseButton::Left),
            capture.left_held,
        );
    } else {
        presentation.cancel_touch_editor_pointer();
    }
    if let Some(layout) = presentation.take_touch_editor_result() {
        if let Some(layout) = layout {
            Arc::make_mut(&mut menu.settings_options).set_touch_layout(layout);
            menu.settings_dirty = true;
            menu.settings_apply = true;
        }
        menu.touch_editor_open = false;
        capture.editor_touch = None;
        capture.left_held = false;
    }
}

/// One finger owns either a continuous control drag or a release-activated toolbar tap.
pub(super) struct Capture {
    id: u64,
    start: [f32; 2],
    action: Option<String>,
    tap: bool,
    cancelled: bool,
}
impl Capture {
    /// Starts continuous dragging immediately; toolbar actions wait for a matching release.
    fn begin(p: &mut UiPresentationRuntime, id: u64, start: [f32; 2]) -> Self {
        let action = p.touch_editor_action_at(start);
        let tap = action.as_deref().is_some_and(|action| {
            !action.starts_with("hud.card:") && !action.starts_with("hud.resize:")
        });
        if !tap {
            p.touch_editor_pointer(start, true, true);
        }
        Self {
            id,
            start,
            action,
            tap,
            cancelled: false,
        }
    }
    /// Returns whether the owner remains held; cancelled taps can never activate on release.
    fn advance(&mut self, p: &mut UiPresentationRuntime, touches: &Touches) -> bool {
        if touches
            .iter_just_canceled()
            .any(|touch| touch.id() == self.id)
        {
            p.cancel_touch_editor_pointer();
            return false;
        }
        let released = touches.get_released(self.id);
        let Some(touch) = released.or_else(|| touches.get_pressed(self.id)) else {
            p.cancel_touch_editor_pointer();
            return false;
        };
        let at = touch.position().to_array();
        if self.tap {
            // Editor toolbar taps use the same eight-logical-pixel drag tolerance at every DPI.
            const TAP_TRAVEL: f32 = 8.;
            self.cancelled |= (at[0] - self.start[0]).hypot(at[1] - self.start[1]) > TAP_TRAVEL;
            if released.is_some() && !self.cancelled && p.touch_editor_action_at(at) == self.action
            {
                p.touch_editor_pointer(at, true, false);
            }
        } else {
            p.touch_editor_pointer(at, false, released.is_none());
        }
        released.is_none()
    }
}

impl MenuRuntime {
    /// A draft owns input only while its settings surface remains unobstructed.
    pub(in crate::menu) fn touch_editor_active(&self) -> bool {
        self.touch_editor_open
            && self.visible
            && self.screen == launcher::menu::MenuScreen::Settings
            && self.dialog.is_none()
            && self.disconnect_message.is_none()
            && !self.is_connecting()
    }
}

#[cfg(test)]
mod tests;
