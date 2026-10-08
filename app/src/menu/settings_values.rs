//! Settings-screen values backed by other app resources: the sound section's
//! volume sliders read and write [`AudioSettings`].

use bevy::prelude::ResMut;

use super::MenuRuntime;
use crate::audio::{AudioCategory, AudioSettings};

/// The sound section's mixer categories; text-to-speech persists without a mixer backend.
pub(crate) const VOLUME_SLIDERS: [(&str, Option<AudioCategory>); 11] = [
    (
        super::settings_options::VOLUME_SETTINGS[0],
        Some(AudioCategory::Master),
    ),
    (
        super::settings_options::VOLUME_SETTINGS[1],
        Some(AudioCategory::Music),
    ),
    (
        super::settings_options::VOLUME_SETTINGS[2],
        Some(AudioCategory::Sound),
    ),
    (
        super::settings_options::VOLUME_SETTINGS[3],
        Some(AudioCategory::Ambient),
    ),
    (
        super::settings_options::VOLUME_SETTINGS[4],
        Some(AudioCategory::Blocks),
    ),
    (
        super::settings_options::VOLUME_SETTINGS[5],
        Some(AudioCategory::Hostile),
    ),
    (
        super::settings_options::VOLUME_SETTINGS[6],
        Some(AudioCategory::Neutral),
    ),
    (
        super::settings_options::VOLUME_SETTINGS[7],
        Some(AudioCategory::Players),
    ),
    (
        super::settings_options::VOLUME_SETTINGS[8],
        Some(AudioCategory::Records),
    ),
    (
        super::settings_options::VOLUME_SETTINGS[9],
        Some(AudioCategory::Weather),
    ),
    (super::settings_options::VOLUME_SETTINGS[10], None),
];
impl MenuRuntime {
    /// A capture's fixed CLI scale, cleared when the native option is changed.
    pub(crate) fn gui_scale_preference(&self) -> Option<u8> {
        self.gui_scale_preference
    }

    #[cfg(test)]
    pub(crate) fn set_gui_scale_preference(&mut self, preference: Option<u8>) {
        self.gui_scale_preference = preference
            .filter(|scale| *scale > 0)
            .map(|scale| scale.clamp(1, 4));
    }

    pub(crate) fn gui_scale_offset(&self) -> i8 {
        self.gui_scale_offset
    }

    pub(super) fn set_gui_scale_offset(&mut self, offset: i8) {
        if self
            .gui_scale_choices
            .iter()
            .any(|choice| choice.offset == offset)
        {
            self.gui_scale_preference = None;
            self.gui_scale_offset = offset;
            self.gui_scale_display_offset = offset;
        }
    }

    /// The native choices track the physical viewport; the saved modifier
    /// survives resize and is clamped when the rendering scale is evaluated.
    pub(crate) fn sync_gui_scale(
        &mut self,
        displayed_offset: i8,
        choices: Vec<ui::DesktopGuiScaleChoice>,
    ) {
        self.gui_scale_display_offset = displayed_offset.clamp(
            choices.first().map_or(0, |choice| choice.offset),
            choices.last().map_or(0, |choice| choice.offset),
        );
        if self.gui_scale_choices != choices {
            self.gui_scale_choices = choices;
        }
    }

    /// A settings press waiting to be applied to the primary window.
    pub(crate) fn take_fullscreen_change(&mut self) -> Option<bool> {
        self.fullscreen_change.take()
    }

    /// Mirror the window without queuing a new settings press.
    pub(crate) fn sync_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen = fullscreen;
    }

    /// Applies the saved sound values to the live mixer.
    pub(crate) fn sync_audio_settings(&mut self, settings: Option<ResMut<AudioSettings>>) {
        let Some(mut settings) = settings else {
            return;
        };
        for (name, category) in VOLUME_SLIDERS {
            if let Some(category) = category {
                let volume = self.settings_options.value(name) as f32 / 100.0;
                if settings.volume(category) != volume {
                    settings.set(category, volume);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> MenuRuntime {
        MenuRuntime::new(true, 2, "Steve".to_owned())
    }

    #[test]
    fn native_gui_scale_choice_clears_the_fixed_cli_override() {
        let mut menu = menu();
        menu.set_gui_scale_preference(Some(2));
        menu.sync_gui_scale(
            0,
            ui::DesktopGuiScale::for_window([1280, 720])
                .choices()
                .collect(),
        );
        menu.activate(super::super::MenuAction::SettingsScale(-1));
        assert_eq!(menu.gui_scale_preference(), None);
        assert_eq!(menu.gui_scale_offset(), -1);
    }

    #[test]
    fn fullscreen_mirroring_does_not_queue_a_setting_change() {
        let mut menu = menu();
        menu.sync_fullscreen(true);
        assert!(menu.view().fullscreen);
        assert_eq!(menu.take_fullscreen_change(), None);
        menu.activate(super::super::MenuAction::SettingsFullscreen(false));
        assert!(!menu.view().fullscreen);
        assert_eq!(menu.take_fullscreen_change(), Some(false));
        assert_eq!(menu.take_fullscreen_change(), None);
    }
}
