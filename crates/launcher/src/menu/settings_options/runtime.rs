//! Converts stored values into the existing subsystem settings.
use std::num::NonZeroU16;

use super::SettingId;
use super::{
    ANIMATIONS_OPTION, FRAME_RATE_UNLIMITED, MOTION_BLUR_OPTION, MOUSE_SENSITIVITY_OPTION,
    SMAA_OPTION, SettingsOptions,
};
use render_api::FrameRateLimit;
use semantic_input::PerspectiveMode;

/// The selected FPS cap, or Unlimited for a value outside the fixed-cap range.
#[must_use]
pub fn frame_rate_limit(value: i32) -> FrameRateLimit {
    match value {
        FRAME_RATE_UNLIMITED.. => FrameRateLimit::Unlimited,
        fixed => u16::try_from(fixed)
            .ok()
            .and_then(NonZeroU16::new)
            .map_or(FrameRateLimit::Unlimited, FrameRateLimit::Fixed),
    }
}

/// The Max Framerate caption: a numeric cap or Unlimited.
pub fn frame_rate_label(value: i32, translate: &dyn Fn(&str) -> String) -> String {
    match frame_rate_limit(value) {
        FrameRateLimit::Unlimited => translate("options.framerateLimit.max"),
        FrameRateLimit::Fixed(fps) => fps.to_string(),
    }
}

impl SettingsOptions {
    /// Builds the existing subsystem settings from the controller's saved values.
    pub fn user_settings(&self) -> ui::UserSettings {
        let mut settings = ui::UserSettings {
            controls: self.controls().unwrap_or_default(),
            ..Default::default()
        };
        settings.video.horizontal_fov_degrees = self.value(SettingId::FieldOfView) as f32;
        settings.video.brightness = self.value(SettingId::Gamma) as f32 / 100.0;
        settings.video.distortion_scale = self.value(SettingId::ScreenDistortion) as f32 / 100.0;
        settings.controls.gamepad_look_sensitivity =
            (self.value(SettingId::ControllerSensitivity) as f32 / 50.0).max(0.01);
        settings.controls.touch_look_sensitivity = (self.value(SettingId::TouchSensitivity) as f32
            / SettingId::TouchSensitivity.definition().default as f32)
            .max(0.01);
        settings.controls.invert_gamepad_y = self.value(SettingId::ControllerInvertYAxis) != 0;
        settings.video.camera_shake = self.value(SettingId::CameraShake) != 0;
        settings.video.damage_bob = self.value(SettingId::DamageBob) as f32 / 100.0;
        settings.video.frame_rate_limit = frame_rate_limit(self.value(SettingId::MaxFramerate));
        settings.video.vsync = self.value(SettingId::Vsync) != 0;
        settings.video.vrr =
            render_api::VrrPreference::from_value(self.value(super::VRR_OPTION.id));
        settings.video.anti_aliasing_samples = self.value(SettingId::Msaa) as u32;
        settings.video.render_scale_percent = self.value(SettingId::RenderResolution) as u8;
        settings.video.render_scale_sharpen = self.value(SettingId::RenderScaleSharpen) != 0;
        settings.video.motion_blur =
            ui::MotionBlurQuality::from_index(self.value(MOTION_BLUR_OPTION.id));
        settings.video.smaa_mode = ui::SmaaMode::from_value(self.value(SMAA_OPTION.id));
        settings.video.render_distance_chunks = self.value(SettingId::RenderDistance) as u8;
        settings.video.chunk_occlusion_culling = self.value(SettingId::ChunkOcclusionCulling) != 0;
        settings.video.view_bobbing =
            ui::ViewBobbing::from_value(self.value(SettingId::ViewBobbing));
        settings.video.java_animations = self.value(ANIMATIONS_OPTION.id) == 0;
        settings.video.show_custom_item_models = self.value(SettingId::ShowCustomItemModels) != 0;
        settings.video.native_skybox_offset =
            self.value(super::NATIVE_SKYBOX_OFFSET_OPTION.id) != 0;
        settings.video.outline_selection = self.value(SettingId::ClassicBoxSelection) != 0;
        settings.video.fov_effects_scale = self.value(SettingId::FieldOfViewToggle) as f32;
        settings.controls.mouse_sensitivity =
            self.value(MOUSE_SENSITIVITY_OPTION.id) as f32 / MOUSE_SENSITIVITY_OPTION.max as f32;
        settings.controls.invert_mouse_y = self.value(SettingId::KeyboardMouseInvertYAxis) != 0;
        settings.gameplay.always_sprint = self.value(SettingId::AlwaysSprint) != 0;
        settings.gameplay.default_perspective = match self.value(SettingId::ThirdPerson) {
            1 => PerspectiveMode::ThirdPersonBack,
            2 => PerspectiveMode::ThirdPersonFront,
            _ => PerspectiveMode::FirstPerson,
        };
        settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_culling_defaults_on_and_survives_save_and_reset() {
        let mut options = SettingsOptions::default();
        assert!(options.user_settings().video.chunk_occlusion_culling);
        assert!(options.set(SettingId::ChunkOcclusionCulling.index(), 0));
        let mut restored = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
        assert!(!restored.user_settings().video.chunk_occlusion_culling);
        restored.reset_group(super::super::SettingsGroup::Video);
        assert!(restored.user_settings().video.chunk_occlusion_culling);
    }

    /// Touch sensitivity survives a save and changes only the touch look multiplier.
    #[test]
    fn touch_sensitivity_is_persisted_and_applied_to_touch_input() {
        let mut options = SettingsOptions::default();
        let original = options.user_settings().controls;
        assert!(options.set(SettingId::TouchSensitivity.index(), 75));
        let restored = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
        let controls = restored.user_settings().controls;
        assert_eq!(controls.touch_look_sensitivity, 1.5);
        assert_eq!(controls.mouse_sensitivity, original.mouse_sensitivity);
        assert_eq!(
            controls.gamepad_look_sensitivity,
            original.gamepad_look_sensitivity
        );
    }
}
