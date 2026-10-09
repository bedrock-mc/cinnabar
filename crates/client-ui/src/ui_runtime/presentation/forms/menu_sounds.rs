//! Authored menu feedback and the native OreUI theme's interaction sounds.

use crate::menu::{MenuAction, MenuScreen};
use launcher::menu::settings_options::{SETTINGS_OPTIONS, SettingKind};

use super::super::UiPresentationRuntime;

/// Retains replay deadlines for the current screen's authored menu sound entries.
#[derive(Default)]
pub(super) struct MenuSoundTimes {
    scope: Option<(MenuScreen, bool)>,
    replays: ReplayTimes<MenuAction>,
}

impl MenuSoundTimes {
    /// Drops deadlines when another screen or modal takes ownership.
    pub(super) fn scope(&mut self, scope: Option<(MenuScreen, bool)>) {
        if self.scope != scope {
            self.scope = scope;
            self.replays.clear();
        }
    }

    /// Admits an entry after its own strict replay interval has elapsed.
    fn admit(&mut self, action: MenuAction, index: usize, now: f64, interval: f64) -> bool {
        self.replays
            .admit(action, index, now, interval, same_control)
    }
}

/// Keeps replay times for authored entries without allocating on idle frames.
pub(super) struct ReplayTimes<A> {
    played: Vec<(A, usize, f64)>,
}

impl<A> Default for ReplayTimes<A> {
    /// Starts without a replay history.
    fn default() -> Self {
        Self { played: Vec::new() }
    }
}

impl<A: Copy> ReplayTimes<A> {
    /// Forgets deadlines when a new screen takes ownership.
    pub(super) fn clear(&mut self) {
        self.played.clear();
    }

    /// Tests the strict interval of one entry on one control.
    pub(super) fn admit(
        &mut self,
        action: A,
        index: usize,
        now: f64,
        interval: f64,
        same: impl Fn(A, A) -> bool,
    ) -> bool {
        if interval <= 0.0 {
            return true;
        }
        if let Some((_, _, last)) = self
            .played
            .iter_mut()
            .find(|(candidate, at, _)| same(*candidate, action) && *at == index)
        {
            if now <= *last + interval {
                return false;
            }
            *last = now;
        } else {
            self.played.push((action, index, now));
        }
        true
    }
}

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

/// Selects one control's entries, preferring its exact current value over a retained value.
pub(super) fn matching<A: Copy + PartialEq>(
    entries: &[(A, json_ui::ControlSound)],
    action: A,
    same: impl Fn(A, A) -> bool,
) -> impl Iterator<Item = (usize, A, &json_ui::ControlSound)> {
    let target = entries
        .iter()
        .find(|(candidate, _)| *candidate == action)
        .or_else(|| {
            entries
                .iter()
                .find(|(candidate, _)| same(*candidate, action))
        })
        .map(|(candidate, _)| *candidate);
    entries
        .iter()
        .filter(move |(candidate, _)| Some(*candidate) == target)
        .enumerate()
        .map(|(index, (candidate, sound))| (index, *candidate, sound))
}

/// Copies shorthand and matching button entries from a control's parsed sound component.
pub(super) fn collect<A: Copy>(
    region: &json_ui::HitRegion,
    action: A,
    out: &mut Vec<(A, json_ui::ControlSound)>,
) {
    let Some(meta) = &region.widget.sounds else {
        return;
    };
    if let Some(sound) = &meta.shorthand {
        out.push((
            action,
            json_ui::ControlSound {
                name: sound.name.clone(),
                volume: sound.volume,
                pitch: sound.pitch,
                min_seconds: 0.0,
            },
        ));
    }
    for entry in &meta.entries {
        if entry
            .button_name
            .as_deref()
            .is_none_or(|button| Some(button) == region.pressed.as_deref())
        {
            out.push((
                action,
                json_ui::ControlSound {
                    name: entry.sound.name.clone(),
                    volume: entry.sound.volume,
                    pitch: entry.sound.pitch,
                    min_seconds: entry.min_seconds_between_plays as f32,
                },
            ));
        }
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
            let mut times = self
                .form_presentation
                .menu_sound_times
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (index, candidate, sound) in
                matching(&self.form_presentation.menu_sounds, action, same_control)
            {
                if times.admit(
                    candidate,
                    index,
                    self.menu_seconds,
                    f64::from(sound.min_seconds),
                ) {
                    crate::sound_requests::ui_sound(&sound.name, sound.volume, sound.pitch);
                }
            }
        }
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
    fn authored_replay_intervals_are_per_control_and_reset_with_the_screen() {
        let mut times = MenuSoundTimes::default();
        times.scope(Some((MenuScreen::Home, false)));
        let a = MenuAction::Navigate(MenuScreen::Settings);
        let b = MenuAction::Navigate(MenuScreen::Servers);
        assert!(times.admit(a, 0, 1.0, 1.0));
        assert!(!times.admit(a, 0, 1.5, 1.0));
        assert!(times.admit(b, 1, 1.5, 1.0));
        assert!(!times.admit(a, 0, 2.0, 1.0));
        assert!(times.admit(a, 0, 2.01, 1.0));
        let index = SETTINGS_OPTIONS
            .iter()
            .position(|option| matches!(option.kind, SettingKind::Toggle))
            .unwrap() as u16;
        let toggle = MenuAction::SettingsOption(index, 0);
        assert!(times.admit(toggle, 2, 1.0, 1.0));
        assert!(!times.admit(MenuAction::SettingsOption(index, 1), 2, 1.5, 1.0));
        times.scope(Some((MenuScreen::Settings, false)));
        assert!(times.admit(a, 0, 2.02, 1.0));
    }

    #[test]
    fn slider_segments_emit_one_controls_entries_and_share_its_deadline() {
        let slider = SETTINGS_OPTIONS
            .iter()
            .position(|option| matches!(option.kind, SettingKind::Slider))
            .unwrap() as u16;
        let sound = |name: &str| json_ui::ControlSound {
            name: name.into(),
            volume: 0.5,
            pitch: 1.25,
            min_seconds: 1.0,
        };
        let entries = vec![
            (MenuAction::SettingsOption(slider, 10), sound("press")),
            (MenuAction::SettingsOption(slider, 10), sound("extra")),
            (MenuAction::SettingsOption(slider, 20), sound("press")),
            (MenuAction::SettingsOption(slider, 20), sound("extra")),
        ];
        let mut times = MenuSoundTimes::default();
        let action = MenuAction::SettingsOption(slider, 10);
        let first: Vec<_> = matching(&entries, action, same_control)
            .filter(|(index, candidate, sound)| {
                times.admit(*candidate, *index, 1.0, f64::from(sound.min_seconds))
            })
            .map(|(_, _, sound)| sound.name.as_str())
            .collect();
        assert_eq!(first, ["press", "extra"]);
        let changed = MenuAction::SettingsOption(slider, 20);
        assert!(
            matching(&entries, changed, same_control).all(|(index, candidate, sound)| !times
                .admit(candidate, index, 1.5, f64::from(sound.min_seconds)))
        );
    }

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
