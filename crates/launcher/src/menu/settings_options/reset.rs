//! Reset groups follow the controls authored in general_section.json.

use super::{
    INVERT_CROSSHAIR_OPTION, SETTINGS_OPTIONS, SettingsOptions, THIRD_PERSON_CROSSHAIR_OPTION,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettingsGroup {
    Video,
    Accessibility,
    Audio,
}

impl SettingsGroup {
    /// Limits resets to registered controls in the authored section, including shared controls.
    fn contains(self, name: &str) -> bool {
        if self == Self::Video
            && [
                THIRD_PERSON_CROSSHAIR_OPTION,
                INVERT_CROSSHAIR_OPTION,
                super::MOTION_BLUR_OPTION,
                super::CHAT_POSITION_OPTION,
            ]
            .iter()
            .any(|option| option.name == name)
        {
            return true;
        }
        match self {
            // P:general_section.json:3000–4058; max_framerate is an inherited slider.
            Self::Video => matches!(
                name,
                "graphics_mode"
                    | "render_distance"
                    | "gamma"
                    | "third_person"
                    | "full_screen"
                    | "hide_hand"
                    | "hide_paperdoll"
                    | "hide_hud"
                    | "screen_animations"
                    | "panorama_speed"
                    | "interface_opacity"
                    | super::SHOW_EXACT_SERVER_PING
                    | super::OREUI_DARK_MODE
                    | "field_of_view"
                    | "show_auto_save_icon"
                    | "classic_box_selection"
                    | "ingame_player_names"
                    | "view_bobbing"
                    | "animations"
                    | "discord_presence"
                    | "camera_shake"
                    | "transparent_leaves"
                    | "bubble_particles"
                    | "render_clouds"
                    | "fancy_skies"
                    | "smooth_lighting"
                    | "field_of_view_toggle"
                    | "damage_bob"
                    | "gui_scale"
                    | "gui_accessibility_scaling"
                    | "max_framerate"
                    | "vsync"
                    | "msaa"
            ),
            // P:general_section.json:4738–5158.
            Self::Accessibility => matches!(
                name,
                "enable_gameplay_subtitles"
                    | "hide_own_gameplay_subtitles"
                    | "hide_ambient_gameplay_subtitles"
                    | "enable_ui_text_to_speech"
                    | "enable_chat_text_to_speech"
                    | "texttospeech_volume"
                    | "enable_open_chat_message"
                    | "hud_text_background_opacity"
                    | "chat_background_opacity"
                    | "actionbar_text_background_opacity"
                    | "camera_shake"
                    | "hide_endflash"
                    | "enable_dithering_blocks"
                    | "enable_dithering_mobs"
                    | "darkness"
                    | "screen_distortion"
                    | "glint_strength"
                    | "glint_speed"
                    | "toast_notification_duration"
                    | "chat_message_duration"
                    | "gui_scale"
                    | "gui_accessibility_scaling"
            ),
            // P:general_section.json:5174–5506; mixer bindings have one shared registry.
            Self::Audio => super::VOLUME_SETTINGS.contains(&name),
        }
    }
}

impl SettingsOptions {
    /// Resets only one section to registry defaults, preserving bindings and other sections.
    pub fn reset_group(&mut self, group: SettingsGroup) -> bool {
        let mut changed = false;
        for (index, option) in SETTINGS_OPTIONS.iter().enumerate() {
            if group.contains(option.name) {
                changed |= self.set(index, option.default);
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Changes a registry option without introducing another copy of its default.
    fn change(options: &mut SettingsOptions, name: &str) -> i32 {
        let index = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
            .unwrap();
        let option = SETTINGS_OPTIONS[index];
        let value = if option.default == option.min {
            option.max
        } else {
            option.min
        };
        assert!(options.set(index, value));
        value
    }

    #[test]
    fn section_reset_restores_defaults_and_preserves_unrelated_settings() {
        for (group, member, unrelated) in [
            (SettingsGroup::Video, "field_of_view", "main_volume"),
            (
                SettingsGroup::Accessibility,
                "chat_background_opacity",
                "field_of_view",
            ),
            (
                SettingsGroup::Audio,
                "main_volume",
                "chat_background_opacity",
            ),
        ] {
            let mut options = SettingsOptions::default();
            change(&mut options, member);
            let retained = change(&mut options, unrelated);
            let key = options.key_control(0);
            assert!(options.reset_group(group));
            let default = SETTINGS_OPTIONS
                .iter()
                .find(|option| option.name == member)
                .unwrap()
                .default;
            assert_eq!(options.value(member), default);
            assert_eq!(options.value(unrelated), retained);
            assert_eq!(options.key_control(0), key);
        }
    }
}
