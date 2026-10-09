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
    pub(super) fn clear(&mut self, presentation: &UiPresentationRuntime) {
        presentation.cancel_menu_sound_touches();
        self.context = None;
        self.touches.clear();
    }

    /// Cancels old touches when a screen or popup takes ownership.
    fn scope(&mut self, context: Option<(MenuScreen, bool)>) {
        if self.context != context {
            self.context = context;
            self.touches.clear();
        }
    }

    /// Plays pointer press sounds without repeating on holds, releases or action dispatch.
    pub(super) fn observe(
        &mut self,
        presentation: &UiPresentationRuntime,
        pointer: Option<UiPoint>,
        pressed: bool,
        touches: &Touches,
        now: f64,
    ) {
        self.scope(presentation.drawn_menu_context());
        if !presentation.uses_native_menu_sounds() {
            if pressed && let Some(point) = pointer {
                presentation.sound_menu_mouse(point, now);
            }
            for touch in touches.iter().chain(touches.iter_just_released()) {
                let position = touch.position();
                presentation.sound_menu_touch(
                    touch.id(),
                    UiPoint::new(position.x, position.y).ok(),
                    touches.just_pressed(touch.id()),
                    touches.get_pressed(touch.id()).is_some(),
                    now,
                );
            }
            if touches.iter_just_canceled().next().is_some() {
                presentation.cancel_menu_sound_touches();
            }
            return;
        }
        if pressed && let Some(action) = pointer.and_then(|point| presentation.hit_test_menu(point))
        {
            presentation.play_menu_sound(action, json_ui::InputMode::Mouse);
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
                presentation.play_menu_sound(action, json_ui::InputMode::Touch);
            }
        }
        if touches.iter_just_canceled().next().is_some() {
            self.touches.clear();
        }
    }
}

/// Text fields and legacy sliders apply their value while touch remains held.
pub(super) fn presses_while_held(action: launcher::menu::MenuAction) -> bool {
    use launcher::menu::{
        MenuAction,
        settings_options::{SETTINGS_OPTIONS, SettingKind},
    };
    action.text_field().is_some()
        || matches!(action, MenuAction::SettingsScale(_))
        || matches!(action, MenuAction::SettingsOption(index, _) if SETTINGS_OPTIONS.get(usize::from(index)).is_some_and(|option| matches!(option.kind, SettingKind::Slider)))
}

/// Sounds the focused control before its keyboard or gamepad activation.
pub(super) fn activate_focused(
    menu: &mut MenuRuntime,
    presentation: &UiPresentationRuntime,
    mode: json_ui::InputMode,
) {
    if let Some(action) = menu.focus_actions().get(menu.focused).copied() {
        presentation.play_menu_sound(menu.live_settings_action(action), mode);
    }
    menu.activate_focused();
}

/// Keeps keyboard auto-repeat from replaying one physical press's feedback.
pub(super) fn activate_key(
    menu: &mut MenuRuntime,
    presentation: &UiPresentationRuntime,
    repeat: bool,
) {
    if repeat {
        menu.activate_focused();
    } else {
        activate_focused(menu, presentation, json_ui::InputMode::Mouse);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use launcher::menu::MenuAction;

    #[test]
    fn popup_ownership_revokes_a_background_touch() {
        let mut sounds = MenuPressSounds::default();
        let action = MenuAction::Navigate(MenuScreen::Servers);
        sounds.scope(Some((MenuScreen::Home, false)));
        sounds
            .touches
            .touch(1, Some(action), [0.0; 2], true, true, 1.0, true);
        sounds.scope(Some((MenuScreen::Home, true)));
        assert_eq!(
            sounds
                .touches
                .touch(1, Some(action), [0.0; 2], false, false, 1.05, true),
            None
        );
    }
}
