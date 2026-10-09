//! Pointer sound edges for JSON-UI chat and the native bed buttons.

use bevy::input::touch::Touches;
use client_ui::{
    sound_requests::{PressSounds, UI_CLICK, ui_sound},
    ui_runtime::presentation::{BedHit, ChatHit, UiPresentationRuntime},
};
use ui::UiPoint;

#[derive(Default)]
pub(crate) struct ChatPressSounds {
    context: Option<(bool, bool)>,
    chat: PressSounds<ChatHit>,
    bed: PressSounds<BedHit>,
}

impl ChatPressSounds {
    /// Revokes touches when another screen owns input.
    pub(super) fn clear(&mut self, presentation: &UiPresentationRuntime) {
        presentation.cancel_chat_sound_touches();
        self.context = None;
        self.chat.clear();
        self.bed.clear();
    }

    /// Sounds native bed presses and returns an accepted pointer activation.
    pub(super) fn bed(
        &mut self,
        presentation: &UiPresentationRuntime,
        pointer: Option<UiPoint>,
        pressed: bool,
        touches: &Touches,
        now: f64,
    ) -> Option<BedHit> {
        self.context = None;
        self.chat.clear();
        presentation.cancel_chat_sound_touches();
        let mut activated = pressed
            .then(|| pointer.and_then(|point| presentation.hit_test_bed(point)))
            .flatten();
        if activated.is_some() {
            ui_sound(UI_CLICK, 1.0, 1.0);
        }
        if touches.iter_just_canceled().next().is_some() {
            self.bed.clear();
        }
        for touch in touches.iter().chain(touches.iter_just_released()) {
            let position = touch.position();
            let hit = UiPoint::new(position.x, position.y)
                .ok()
                .and_then(|point| presentation.hit_test_bed(point));
            let (released, sounded) = self.bed_touch(
                touch.id(),
                hit,
                [position.x, position.y],
                touches.just_pressed(touch.id()),
                touches.get_pressed(touch.id()).is_some(),
                now,
            );
            activated = activated.or(released);
            if sounded {
                ui_sound(UI_CLICK, 1.0, 1.0);
            }
        }
        activated
    }

    /// Returns bed activation and feedback from the same accepted touch edge.
    #[allow(clippy::too_many_arguments)]
    fn bed_touch(
        &mut self,
        id: u64,
        hit: Option<BedHit>,
        point: [f32; 2],
        pressed: bool,
        held: bool,
        now: f64,
    ) -> (Option<BedHit>, bool) {
        if pressed && !held {
            self.bed.touch(id, hit, point, true, true, now, true);
        }
        let activated = (!held)
            .then(|| self.bed.released_action(id, point))
            .flatten()
            .filter(|captured| hit == Some(*captured));
        let sounded = self
            .bed
            .touch(id, hit, point, pressed, held, now, true)
            .is_some();
        (activated, sounded)
    }

    /// Sounds mouse presses and accepted JSON-UI touch releases before dispatch.
    pub(super) fn chat(
        &mut self,
        presentation: &UiPresentationRuntime,
        pointer: Option<UiPoint>,
        pressed: bool,
        touches: &Touches,
        now: f64,
        actions: &mut Vec<UiPoint>,
    ) {
        self.bed.clear();
        let context = (
            presentation.chat_settings_open(),
            presentation.chat_link_confirmation_open(),
        );
        if self.context.replace(context) != Some(context) {
            self.chat.clear();
        }
        if pressed && let Some(point) = pointer {
            presentation.sound_chat_mouse(point, now);
            actions.push(point);
        }
        if touches.iter_just_canceled().next().is_some() {
            self.chat.clear();
            presentation.cancel_chat_sound_touches();
        }
        for touch in touches.iter().chain(touches.iter_just_released()) {
            let position = touch.position();
            let point = UiPoint::new(position.x, position.y).ok();
            let hit = point.and_then(|point| presentation.hit_test_chat(point));
            presentation.sound_chat_touch(
                touch.id(),
                point,
                touches.just_pressed(touch.id()),
                touches.get_pressed(touch.id()).is_some(),
                now,
            );
            if let Some(captured) = self.chat.touch(
                touch.id(),
                hit,
                [position.x, position.y],
                touches.just_pressed(touch.id()),
                touches.get_pressed(touch.id()).is_some(),
                now,
                false,
            ) && hit == Some(captured)
            {
                if let Some(point) = point {
                    actions.push(point);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bed_tap_with_both_edges_activates_and_sounds_once() {
        let mut sounds = ChatPressSounds::default();
        assert_eq!(
            sounds.bed_touch(1, Some(BedHit::LeaveBed), [0.0; 2], true, false, 1.0),
            (Some(BedHit::LeaveBed), true)
        );
        assert_eq!(
            sounds.bed_touch(1, Some(BedHit::LeaveBed), [0.0; 2], false, false, 1.0),
            (None, false)
        );
    }

    #[test]
    fn a_sounded_bed_hold_activates_on_release_without_another_sound() {
        let mut sounds = ChatPressSounds::default();
        let hit = Some(BedHit::LeaveBed);
        assert_eq!(
            sounds.bed_touch(1, hit, [0.0; 2], true, true, 1.0),
            (None, false)
        );
        assert_eq!(
            sounds.bed_touch(1, hit, [0.0; 2], false, true, 1.2),
            (None, true)
        );
        assert_eq!(
            sounds.bed_touch(1, hit, [0.0; 2], false, false, 1.3),
            (hit, false)
        );
    }
}
