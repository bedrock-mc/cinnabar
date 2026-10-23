//! World-resolution help and saved percentage settings.

pub const DESCRIPTION: &str = "Lower values improve frame rate by rendering the world at a lower resolution. Menus and the HUD stay at native resolution. Applies to a single full-window game view.";

#[cfg(test)]
mod tests {
    use crate::menu::settings_options::{
        SettingId, SettingsGroup, SettingsOptions, video_presets::VideoPreset,
    };

    #[test]
    fn native_resolution_defaults_for_new_and_existing_settings() {
        assert_eq!(
            ui::VideoSettings::default().render_scale_percent,
            ui::DEFAULT_RENDER_SCALE_PERCENT
        );
        for bytes in [b"{}".as_slice(), br#"{"values":{"msaa":4}}"#] {
            let saved = SettingsOptions::decode(bytes).unwrap();
            assert_eq!(
                saved.user_settings().video.render_scale_percent,
                ui::DEFAULT_RENDER_SCALE_PERCENT
            );
        }
    }

    #[test]
    fn every_integer_resolution_round_trips_and_invalid_percentages_stay_bounded() {
        let index = SettingId::RenderResolution.index();
        for expected in ui::MIN_RENDER_SCALE_PERCENT..=ui::MAX_RENDER_SCALE_PERCENT {
            let mut saved = SettingsOptions::default();
            saved.set(index, i32::from(expected));
            let loaded = SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
            assert_eq!(loaded.user_settings().video.render_scale_percent, expected);
        }
        for (percent, expected) in [
            (i32::MIN, ui::MIN_RENDER_SCALE_PERCENT),
            (-1, ui::MIN_RENDER_SCALE_PERCENT),
            (0, ui::MIN_RENDER_SCALE_PERCENT),
            (1, ui::MIN_RENDER_SCALE_PERCENT),
            (2, ui::MIN_RENDER_SCALE_PERCENT),
            (3, ui::MIN_RENDER_SCALE_PERCENT),
            (4, ui::MIN_RENDER_SCALE_PERCENT),
            (9, ui::MIN_RENDER_SCALE_PERCENT),
            (101, ui::MAX_RENDER_SCALE_PERCENT),
            (i32::MAX, ui::MAX_RENDER_SCALE_PERCENT),
        ] {
            let json = format!(r#"{{"values":{{"render_resolution":{percent}}}}}"#);
            let loaded = SettingsOptions::decode(json.as_bytes()).unwrap();
            assert_eq!(loaded.user_settings().video.render_scale_percent, expected);
        }
    }

    #[test]
    fn resolution_keyboard_steps_are_one_percent_and_stop_at_the_endpoints() {
        let index = SettingId::RenderResolution.index();
        let mut saved = SettingsOptions::default();
        for (start, direction, expected) in [
            (37, 1, 38),
            (37, -1, 36),
            (
                i32::from(ui::MIN_RENDER_SCALE_PERCENT),
                -1,
                ui::MIN_RENDER_SCALE_PERCENT,
            ),
            (
                i32::from(ui::MAX_RENDER_SCALE_PERCENT),
                1,
                ui::MAX_RENDER_SCALE_PERCENT,
            ),
        ] {
            saved.set(index, start);
            saved.set(index, saved.offset_value(index, direction));
            assert_eq!(saved.user_settings().video.render_scale_percent, expected);
        }
    }

    #[test]
    fn video_reset_restores_native_resolution_and_presets_preserve_the_players_choice() {
        let mut saved = SettingsOptions::default();
        saved.set(SettingId::RenderResolution.index(), 37);
        for preset in VideoPreset::ALL {
            saved.apply_video_preset(preset);
            assert_eq!(saved.user_settings().video.render_scale_percent, 37);
        }
        assert!(saved.reset_group(SettingsGroup::Video));
        assert_eq!(
            saved.user_settings().video.render_scale_percent,
            ui::DEFAULT_RENDER_SCALE_PERCENT
        );
    }
    #[test]
    fn sharpening_persists_and_resets_with_video_settings() {
        let mut saved = SettingsOptions::default();
        assert_eq!(
            saved.user_settings().video.render_scale_sharpen,
            ui::DEFAULT_RENDER_SCALE_SHARPEN
        );
        saved.set(SettingId::RenderResolution.index(), 37);
        saved.set(SettingId::RenderScaleSharpen.index(), 0);
        let mut loaded = SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(loaded.user_settings().video.render_scale_percent, 37);
        assert!(!loaded.user_settings().video.render_scale_sharpen);
        loaded.reset_group(SettingsGroup::Video);
        assert_eq!(
            loaded.user_settings().video.render_scale_percent,
            ui::DEFAULT_RENDER_SCALE_PERCENT
        );
        assert_eq!(
            loaded.user_settings().video.render_scale_sharpen,
            ui::DEFAULT_RENDER_SCALE_SHARPEN
        );
    }
}
