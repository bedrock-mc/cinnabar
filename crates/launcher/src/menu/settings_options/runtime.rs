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
        settings.controls.invert_gamepad_y = self.value(SettingId::ControllerInvertYAxis) != 0;
        settings.video.camera_shake = self.value(SettingId::CameraShake) != 0;
        settings.video.damage_bob = self.value(SettingId::DamageBob) as f32 / 100.0;
        settings.video.frame_rate_limit = frame_rate_limit(self.value(SettingId::MaxFramerate));
        settings.video.vsync = self.value(SettingId::Vsync) != 0;
        settings.video.vrr =
            render_api::VrrPreference::from_value(self.value(super::VRR_OPTION.id));
        settings.video.anti_aliasing_samples = self.value(SettingId::Msaa) as u32;
        settings.video.motion_blur =
            ui::MotionBlurQuality::from_index(self.value(MOTION_BLUR_OPTION.id));
        settings.video.smaa_mode = ui::SmaaMode::from_value(self.value(SMAA_OPTION.id));
        settings.video.render_distance_chunks = self.value(SettingId::RenderDistance) as u8;
        settings.video.view_bobbing =
            ui::ViewBobbing::from_value(self.value(SettingId::ViewBobbing));
        settings.video.java_animations = self.value(ANIMATIONS_OPTION.id) == 0;
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
