//! Sound edges are observed once, independently of when controls activate.

use bevy::input::touch::Touches;
use client_ui::{sound_requests::PressSounds, ui_runtime::presentation::UiPresentationRuntime};
use ui::UiPoint;

use super::super::{MenuRuntime, MenuScreen};

#[derive(Default)]
pub(super) struct MenuPressSounds {
    context: Option<(MenuScreen, bool)>,
    touches: PressSounds,
}

impl MenuPressSounds {
    /// Revokes retained touch presses when a menu loses input ownership.
    pub(super) fn clear(&mut self) {
        self.context = None;
        self.touches.clear();
    }

    /// Plays pointer press sounds without repeating on holds, releases or action dispatch.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn observe(
        &mut self,
        menu: &MenuRuntime,
        presentation: &UiPresentationRuntime,
        pointer: Option<UiPoint>,
        pressed: bool,
        touches: &Touches,
        now: f64,
    ) {
        let context = (menu.screen(), menu.dialog.is_some());
        if self.context.replace(context) != Some(context) {
            self.touches.clear();
        }
        if pressed && let Some(action) = pointer.and_then(|point| presentation.hit_test_menu(point))
        {
            presentation.play_menu_sound(action);
        }
        for touch in touches.iter().chain(touches.iter_just_released()) {
            let position = touch.position();
            let action = UiPoint::new(position.x, position.y)
                .ok()
                .and_then(|point| presentation.hit_test_menu(point));
            if let Some(action) = self.touches.touch(
                touch.id(),
                action,
                [position.x, position.y],
                touches.just_pressed(touch.id()),
                touches.get_pressed(touch.id()).is_some(),
                now,
                presentation.uses_native_menu_sounds(),
            ) {
                presentation.play_menu_sound(action);
            }
        }
        if touches.iter_just_canceled().next().is_some() {
            self.touches.clear();
        }
    }
}

/// Sounds the focused control before its keyboard or gamepad activation.
pub(super) fn activate_focused(menu: &mut MenuRuntime, presentation: &UiPresentationRuntime) {
    if let Some(action) = menu.focus_actions().get(menu.focused).copied() {
        presentation.play_menu_sound(menu.live_settings_action(action));
    }
    menu.activate_focused();
}
