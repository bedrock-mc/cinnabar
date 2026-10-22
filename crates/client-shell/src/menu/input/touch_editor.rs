//! Settings-owned pointer capture prevents editing from activating gameplay or other menus.
use super::*;
use bevy::input::touch::{TouchInput, TouchPhase};
use std::sync::Arc;

/// Delivers only one owning finger; its cancellation rolls back the unfinished drag.
pub(super) fn drive(
    menu: &mut MenuRuntime,
    presentation: &mut UiPresentationRuntime,
    touches: &Touches,
    events: &[TouchInput],
    capture: &mut GuiScaleDrag,
    pointer: Option<[f32; 2]>,
    buttons: &ButtonInput<MouseButton>,
) {
    for event in events {
        let at = event.position.to_array();
        if event.phase == TouchPhase::Started
            && !capture.left_held
            && capture
                .editor_touch
                .as_ref()
                .is_none_or(|owner| owner.id == event.id)
        {
            presentation.cancel_touch_editor_pointer();
            capture.editor_touch = Some(Capture::begin(presentation, event.id, at));
        } else if let Some(owner) = capture
            .editor_touch
            .as_mut()
            .filter(|owner| owner.id == event.id)
            && !owner.event(presentation, event.phase, at)
        {
            capture.editor_touch = None;
        }
    }
    if let Some(owner) = capture.editor_touch.as_ref() {
        if touches.get_pressed(owner.id).is_none() {
            presentation.cancel_touch_editor_pointer();
            capture.editor_touch = None;
        } else if !owner.tap {
            presentation.touch_editor_pointer(owner.last, false, true);
        }
    } else if events.is_empty() && touches.iter().next().is_none() {
        if let Some(at) = pointer {
            presentation.touch_editor_pointer(
                at,
                buttons.just_pressed(MouseButton::Left),
                capture.left_held,
            );
        } else {
            presentation.cancel_touch_editor_pointer();
        }
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
    last: [f32; 2],
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
            last: start,
            action,
            tap,
            cancelled: false,
        }
    }
    /// Applies the actual ordered native sample, including the final release coordinates.
    fn event(&mut self, p: &mut UiPresentationRuntime, phase: TouchPhase, at: [f32; 2]) -> bool {
        if phase == TouchPhase::Canceled {
            p.cancel_touch_editor_pointer();
            return false;
        }
        self.last = at;
        let released = phase == TouchPhase::Ended;
        if self.tap {
            // Editor toolbar taps use an eight-logical-pixel drag tolerance at every DPI.
            const TAP_TRAVEL: f32 = 8.;
            self.cancelled |= (at[0] - self.start[0]).hypot(at[1] - self.start[1]) > TAP_TRAVEL;
            if released && !self.cancelled && p.touch_editor_action_at(at) == self.action {
                p.touch_editor_pointer(at, true, false);
            }
        } else {
            p.touch_editor_pointer(at, false, !released);
        }
        !released
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
