//! Authored menu feedback and the native OreUI theme's interaction sounds.

use crate::menu::{MenuAction, MenuScreen};
use launcher::menu::settings_options::{SETTINGS_OPTIONS, SettingKind};

use super::super::UiPresentationRuntime;

/// Compares a control across value changes without treating different options as one.
pub(super) fn same_control(a: MenuAction, b: MenuAction) -> bool {
    match (a, b) {
        (MenuAction::SettingsOption(a, av), MenuAction::SettingsOption(b, bv)) => {
            a == b
                && (av == bv
                    || SETTINGS_OPTIONS.get(usize::from(a)).is_some_and(|option| {
                        matches!(option.kind, SettingKind::Slider | SettingKind::Toggle)
                    }))
        }
        (MenuAction::SettingsScale(_), MenuAction::SettingsScale(_)) => true,
        (MenuAction::SettingsFullscreen(_), MenuAction::SettingsFullscreen(_)) => true,
        _ => a == b,
    }
}

/// Tracks the drawer's open state; its first paint does not play a sound.
#[derive(Default)]
pub(super) struct DrawerSounds(Option<bool>);

impl DrawerSounds {
    /// Returns the sound for a changed drawer state, once per transition.
    pub(super) fn update(&mut self, open: bool) -> Option<&'static str> {
        let previous = self.0.replace(open)?;
        (previous != open).then_some(if open {
            "ui.drawer_open"
        } else {
            "ui.drawer_close"
        })
    }
}

/// Native sliders and text fields are silent; other enabled controls use the theme click.
fn native_sound(action: MenuAction, scale_picker: bool) -> Option<&'static str> {
    if action.text_field().is_some()
        || matches!(action, MenuAction::SettingsScale(_) if !scale_picker)
        || matches!(action, MenuAction::SettingsOption(index, _) if SETTINGS_OPTIONS.get(usize::from(index))
            .is_some_and(|option| matches!(option.kind, SettingKind::Slider)))
    {
        return None;
    }
    Some(crate::sound_requests::UI_CLICK)
}

impl UiPresentationRuntime {
    /// Whether the current menu uses the native theme's press sounds.
    pub fn uses_native_menu_sounds(&self) -> bool {
        self.form_presentation.native_menu_sounds
    }

    /// Plays one control interaction through the pack-aware named audio queue.
    pub fn play_menu_sound(&self, action: MenuAction) {
        if self.uses_native_menu_sounds() {
            let scale_picker = self
                .menu_view
                .as_ref()
                .is_some_and(|view| view.settings_scale_picker);
            if let Some(name) = native_sound(action, scale_picker) {
                crate::sound_requests::ui_sound(name, 1.0, 1.0);
            }
        } else {
            let state = &self.form_presentation;
            if let Some((_, key)) = state
                .menu_keys
                .iter()
                .find(|(candidate, _)| same_control(*candidate, action))
            {
                state
                    .menu_audio
                    .activate(key, self.menu_seconds, crate::sound_requests::ui_sound);
            }
        }
    }

    /// Routes an actual pointer press through the painted JSON-UI controls.
    pub fn sound_menu_mouse(&self, point: ui::UiPoint, now: f64) {
        self.form_presentation
            .menu_audio
            .mouse(point, now, crate::sound_requests::ui_sound);
    }

    /// Retains JSON-UI touch identity and sounds only an accepted release.
    pub fn sound_menu_touch(
        &self,
        id: u64,
        point: Option<ui::UiPoint>,
        pressed: bool,
        held: bool,
        now: f64,
    ) {
        self.form_presentation.menu_audio.touch(
            id,
            point,
            pressed,
            held,
            now,
            crate::sound_requests::ui_sound,
        );
    }

    /// Cancels touches when another input owner takes control.
    pub fn cancel_menu_sound_touches(&self) {
        self.form_presentation.menu_audio.cancel_touches();
    }

    /// Observes the visible drawer after a successful menu build.
    pub(super) fn observe_menu_drawer(&mut self, screen: Option<MenuScreen>) {
        if let Some(name) = self
            .form_presentation
            .drawer_sounds
            .update(screen == Some(MenuScreen::Friends))
        {
            crate::sound_requests::ui_sound(name, 1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_buttons_toggles_tabs_and_dropdowns_use_the_theme_click() {
        let toggle = SETTINGS_OPTIONS
            .iter()
            .position(|option| matches!(option.kind, SettingKind::Toggle))
            .unwrap() as u16;
        for action in [
            MenuAction::Navigate(MenuScreen::Settings),
            MenuAction::Navigate(MenuScreen::Servers),
            MenuAction::SettingsOption(toggle, 1),
            MenuAction::SettingsDropdown(toggle),
            MenuAction::DressingRoom(launcher::dressing_room::Action::Select(0)),
        ] {
            assert_eq!(
                native_sound(action, false),
                Some(crate::sound_requests::UI_CLICK)
            );
        }
    }

    #[test]
    fn native_slider_motion_and_text_focus_are_silent() {
        let slider = SETTINGS_OPTIONS
            .iter()
            .position(|option| matches!(option.kind, SettingKind::Slider))
            .unwrap() as u16;
        for action in [
            MenuAction::AddName,
            MenuAction::SettingsScale(1),
            MenuAction::SettingsOption(slider, 50),
        ] {
            assert_eq!(native_sound(action, false), None);
        }
        assert_eq!(
            native_sound(MenuAction::SettingsScale(1), true),
            Some(crate::sound_requests::UI_CLICK)
        );
    }

    #[test]
    fn drawer_open_and_close_emit_once_without_replaying_paints() {
        let mut drawer = DrawerSounds::default();
        assert_eq!(drawer.update(false), None);
        assert_eq!(drawer.update(true), Some("ui.drawer_open"));
        assert_eq!(drawer.update(true), None);
        assert_eq!(drawer.update(false), Some("ui.drawer_close"));
        assert_eq!(drawer.update(false), None);
        let (_, allocations) = crate::allocation_count::count(|| {
            for _ in 0..100 {
                assert_eq!(drawer.update(false), None);
            }
        });
        assert_eq!(allocations, 0);
    }
}
