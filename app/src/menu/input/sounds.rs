//! Sound edges are observed once, independently of when controls activate.

use bevy::input::touch::Touches;
use client_ui::{sound_requests::PressSounds, ui_runtime::presentation::UiPresentationRuntime};
use ui::UiPoint;

use super::super::{MenuRuntime, MenuScreen};

#[derive(Default)]
pub(super) struct MenuPressSounds {
    context: Option<(MenuScreen, bool)>,
    touches: PressSounds,
    released:
        [Option<(u64, launcher::menu::MenuAction)>; client_ui::sound_requests::MAX_UI_TOUCHES],
}

impl MenuPressSounds {
    /// Revokes retained touch presses when a menu loses input ownership.
    pub(super) fn clear(&mut self, presentation: &UiPresentationRuntime) {
        presentation.cancel_menu_sound_touches();
        self.context = None;
        self.touches.clear();
        self.released.fill(None);
    }

    /// Cancels old touches when a screen or popup takes ownership.
    fn scope(&mut self, context: Option<(MenuScreen, bool)>) {
        if self.context != context {
            self.context = context;
            self.touches.clear();
        }
    }

    /// Returns a release accepted by the same capture that owns its feedback.
    pub(super) fn released_action(&self, id: u64) -> Option<launcher::menu::MenuAction> {
        self.released
            .iter()
            .flatten()
            .find_map(|(captured, action)| (*captured == id).then_some(*action))
    }

    /// Retains accepted releases for the action adapter without allocating.
    fn record_release(&mut self, id: u64, action: launcher::menu::MenuAction) {
        if let Some(slot) = self.released.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some((id, action));
        }
    }

    /// Plays feedback and retains only releases accepted by their original control.
    pub(super) fn observe(
        &mut self,
        presentation: &UiPresentationRuntime,
        pointer: Option<UiPoint>,
        pressed: bool,
        touches: &Touches,
        now: f64,
    ) {
        self.released.fill(None);
        self.scope(presentation.drawn_menu_context());
        let native = presentation.uses_native_menu_sounds();
        for touch in touches.iter_just_canceled() {
            self.touches.cancel_touch(touch.id());
            presentation.cancel_menu_sound_touch(touch.id());
        }
        if pressed && let Some(point) = pointer {
            if native {
                if let Some(action) = presentation.hit_test_menu(point) {
                    presentation.play_menu_sound(action, json_ui::InputMode::Mouse);
                }
            } else {
                presentation.sound_menu_mouse(point, now);
            }
        }
        for touch in touches
            .iter_just_pressed()
            .filter(|touch| !touches.just_canceled(touch.id()))
        {
            let position = touch.position();
            let point = UiPoint::new(position.x, position.y).ok();
            if native {
                let action = point.and_then(|point| presentation.hit_test_menu(point));
                self.touches.touch(
                    touch.id(),
                    action,
                    [position.x, position.y],
                    true,
                    true,
                    now,
                    action.is_some_and(|action| presentation.menu_touch_uses_press_slop(action)),
                );
            } else {
                presentation.sound_menu_touch(touch.id(), point, true, true, now);
            }
        }
        for touch in touches.iter().chain(touches.iter_just_released()) {
            let position = touch.position();
            let point = UiPoint::new(position.x, position.y).ok();
            let action = point.and_then(|point| presentation.hit_test_menu(point));
            let held = touches.get_pressed(touch.id()).is_some();
            if native {
                let captured = self
                    .touches
                    .released_action(touch.id(), [position.x, position.y]);
                let accepted =
                    captured.and_then(|captured| super::native_release_action(captured, action));
                if !held && let Some(action) = accepted {
                    self.record_release(touch.id(), action);
                }
                let sound_action = if accepted.is_some() { captured } else { action };
                if let Some(action) = self.touches.touch(
                    touch.id(),
                    sound_action,
                    [position.x, position.y],
                    false,
                    held,
                    now,
                    false,
                ) {
                    presentation.play_menu_sound(action, json_ui::InputMode::Touch);
                }
            } else if presentation.sound_menu_touch(touch.id(), point, false, held, now)
                && let Some(action) = action
            {
                self.record_release(touch.id(), action);
            }
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

    /// Builds a native home screen and finds an interior point on its Settings button.
    fn home_touch_app() -> (
        bevy::prelude::App,
        bevy::prelude::Entity,
        bevy::prelude::Vec2,
    ) {
        use bevy::input::InputPlugin;
        use client_ui::{test_support::fixture_font, ui_runtime::UiRuntime};
        let menu = MenuRuntime::new(true, 2, "test".into());
        let player = crate::player_runtime::PlayerRuntime::new(1);
        let mut presentation = UiPresentationRuntime::new(fixture_font()).unwrap();
        presentation.set_menu_view(Some(menu.view()));
        presentation
            .build(
                &player,
                &UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(presentation.uses_native_menu_sounds());
        let target = Some(MenuAction::Navigate(MenuScreen::Settings));
        let point = (0..720)
            .step_by(8)
            .flat_map(|y| (0..1280).step_by(8).map(move |x| [x as f32, y as f32]))
            .find(|point| {
                [0.0, 8.0].into_iter().all(|dx| {
                    presentation.hit_test_menu(UiPoint::new(point[0] + dx, point[1]).unwrap())
                        == target
                })
            })
            .unwrap();
        let mut app = bevy::prelude::App::new();
        app.add_plugins(InputPlugin)
            .insert_resource(player)
            .insert_resource(menu)
            .insert_resource(presentation)
            .insert_resource(crate::menu::MenuClipboard::with_access(|_| None, |_| {}))
            .add_systems(bevy::prelude::Update, crate::menu::drive_menu_input);
        let window = app
            .world_mut()
            .spawn((
                bevy::prelude::Window {
                    focused: true,
                    ..Default::default()
                },
                bevy::window::CursorOptions::default(),
                bevy::window::PrimaryWindow,
            ))
            .id();
        (app, window, bevy::prelude::Vec2::new(point[0], point[1]))
    }

    /// Sends a deterministic touch event through the input adapter without OS input.
    fn touch(
        app: &mut bevy::prelude::App,
        window: bevy::prelude::Entity,
        phase: bevy::input::touch::TouchPhase,
        point: bevy::prelude::Vec2,
    ) {
        app.world_mut()
            .write_message(bevy::input::touch::TouchInput {
                phase,
                position: point,
                window,
                force: None,
                id: 1,
            });
    }

    #[test]
    fn cancelled_menu_touch_does_not_navigate() {
        use bevy::{input::touch::TouchPhase, prelude::Vec2};
        for case in ["inside", "return", "one frame"] {
            let (mut app, window, point) = home_touch_app();
            touch(&mut app, window, TouchPhase::Started, point);
            if case != "one frame" {
                app.update();
            }
            let moved = if case == "return" {
                Vec2::new(-100.0, -100.0)
            } else {
                point + Vec2::new(8.0, 0.0)
            };
            touch(&mut app, window, TouchPhase::Moved, moved);
            if case != "one frame" {
                app.update();
            }
            let released = if case == "return" { point } else { moved };
            if case == "return" {
                touch(&mut app, window, TouchPhase::Moved, point);
                app.update();
            }
            touch(&mut app, window, TouchPhase::Ended, released);
            app.update();
            assert_eq!(
                app.world().resource::<MenuRuntime>().screen(),
                MenuScreen::Home,
                "cancelled {case} gesture"
            );
        }
    }

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
