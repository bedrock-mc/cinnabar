//! Converts stored values into the existing subsystem settings.
use super::{ANIMATIONS_OPTION, MOTION_BLUR_OPTION, MOUSE_SENSITIVITY_OPTION, SettingsOptions};
use semantic_input::PerspectiveMode;

impl SettingsOptions {
    /// Builds the existing subsystem settings from the controller's saved values.
    pub fn user_settings(&self) -> ui::UserSettings {
        let mut settings = ui::UserSettings {
            controls: self.controls().unwrap_or_default(),
            ..Default::default()
        };
        settings.video.horizontal_fov_degrees = self.value("field_of_view") as f32;
        settings.video.brightness = self.value("gamma") as f32 / 100.0;
        settings.video.distortion_scale = self.value("screen_distortion") as f32 / 100.0;
        settings.controls.gamepad_look_sensitivity =
            (self.value("controller_sensitivity") as f32 / 50.0).max(0.01);
        settings.controls.invert_gamepad_y = self.value("controller_invert_y_axis") != 0;
        settings.video.camera_shake = self.value("camera_shake") != 0;
        settings.video.damage_bob = self.value("damage_bob") as f32 / 100.0;
        settings.video.frame_cap =
            (self.value("max_framerate") != 0).then(|| self.value("max_framerate") as u16);
        settings.video.vsync = self.value("vsync") != 0;
        settings.video.anti_aliasing_samples = self.value("msaa") as u32;
        settings.video.motion_blur =
            ui::MotionBlurQuality::from_index(self.value(MOTION_BLUR_OPTION.name));
        settings.video.render_distance_chunks = self.value("render_distance") as u8;
        settings.video.view_bobbing = self.value("view_bobbing") != 0;
        settings.video.java_animations = self.value(ANIMATIONS_OPTION.name) == 0;
        settings.video.outline_selection = self.value("classic_box_selection") != 0;
        settings.video.fov_effects_scale = self.value("field_of_view_toggle") as f32;
        settings.controls.mouse_sensitivity =
            self.value(MOUSE_SENSITIVITY_OPTION.name) as f32 / MOUSE_SENSITIVITY_OPTION.max as f32;
        settings.controls.invert_mouse_y = self.value("keyboard_mouse_invert_y_axis") != 0;
        settings.gameplay.always_sprint = self.value("always_sprint") != 0;
        settings.gameplay.default_perspective = match self.value("third_person") {
            1 => PerspectiveMode::ThirdPersonBack,
            2 => PerspectiveMode::ThirdPersonFront,
            _ => PerspectiveMode::FirstPerson,
        };
        settings
    }
}
