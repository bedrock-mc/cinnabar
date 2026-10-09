//! Pointer sound edges for JSON-UI chat and the native bed buttons.

use bevy::input::touch::Touches;
use client_ui::{
    sound_requests::{PressSounds, UI_CLICK, ui_sound},
    ui_runtime::presentation::{BedHit, ChatHit, UiPresentationRuntime},
};
use ui::UiPoint;

#[derive(Default)]
pub(super) struct ChatPressSounds {
    context: Option<(bool, bool)>,
    chat: PressSounds<ChatHit>,
    bed: PressSounds<BedHit>,
}

impl ChatPressSounds {
    /// Revokes touches when another screen owns input.
    pub(super) fn clear(&mut self) {
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
            if touches.just_released(touch.id()) {
                activated = self
                    .bed
                    .released_action(touch.id(), [position.x, position.y])
                    .filter(|captured| hit == Some(*captured));
            }
            if self
                .bed
                .touch(
                    touch.id(),
                    hit,
                    [position.x, position.y],
                    touches.just_pressed(touch.id()),
                    touches.get_pressed(touch.id()).is_some(),
                    now,
                    true,
                )
                .is_some()
            {
                ui_sound(UI_CLICK, 1.0, 1.0);
            }
        }
        activated
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
            if let Some(hit) = presentation.hit_test_chat(point) {
                presentation.play_chat_sound(hit, now);
            }
            actions.push(point);
        }
        if touches.iter_just_canceled().next().is_some() {
            self.chat.clear();
        }
        for touch in touches.iter().chain(touches.iter_just_released()) {
            let position = touch.position();
            let point = UiPoint::new(position.x, position.y).ok();
            let hit = point.and_then(|point| presentation.hit_test_chat(point));
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
                presentation.play_chat_sound(captured, now);
                if let Some(point) = point {
                    actions.push(point);
                }
            }
        }
    }
}
