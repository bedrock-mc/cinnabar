//! One handoff applies menu edits to the camera, input, window and sound authorities.

use std::sync::Arc;

use bevy::prelude::ResMut;
use semantic_input::PerspectiveMode;

use super::{SETTINGS_OPTIONS, SettingsOptions, persistence::SETTINGS_FILE};
use crate::{menu::MenuRuntime, settings_runtime::RuntimeSettings};

impl SettingsOptions {
    /// Builds the existing subsystem settings from the controller's saved values.
    pub(crate) fn user_settings(&self) -> ui::UserSettings {
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
        settings.video.render_distance_chunks = self.value("render_distance") as u8;
        settings.video.view_bobbing = self.value("view_bobbing") != 0;
        settings.video.outline_selection = self.value("classic_box_selection") != 0;
        settings.video.fov_effects_scale = self.value("field_of_view_toggle") as f32;
        settings.controls.mouse_sensitivity =
            (self.value("keyboard_mouse_sensitivity") as f32 / 50.0).max(0.01);
        settings.controls.invert_mouse_y = self.value("keyboard_mouse_invert_y_axis") != 0;
        settings.gameplay.default_perspective = match self.value("third_person") {
            1 => PerspectiveMode::ThirdPersonBack,
            2 => PerspectiveMode::ThirdPersonFront,
            _ => PerspectiveMode::FirstPerson,
        };
        settings
    }
}

impl MenuRuntime {
    /// Publishes normalized glint factors to the shared UI renderer.
    pub(crate) fn ui_glint_settings(&self) -> render::UiGlintSettings {
        render::UiGlintSettings {
            strength: self.settings_options.value("glint_strength") as f32 / 100.0,
            speed: self.settings_options.value("glint_speed") as f32 / 100.0,
        }
    }

    /// Reads the desktop scoping option for the device that produced this frame's turn.
    pub(crate) fn spyglass_damping(&self, mode: semantic_input::InputMode) -> f32 {
        let name = match mode {
            semantic_input::InputMode::KeyboardMouse => "spyglass_mouse_dampening",
            semantic_input::InputMode::GamePad => "spyglass_gamepad_dampening",
            semantic_input::InputMode::Touch => return 0.0,
        };
        self.settings_options.value(name) as f32 / 100.0
    }

    /// Applies a validated setting edit and marks its persistence and runtime handoff dirty.
    pub(in crate::menu) fn set_option(&mut self, index: u16, value: i32) {
        if Arc::make_mut(&mut self.settings_options).set(usize::from(index), value) {
            self.settings_dirty = true;
            self.settings_apply = true;
        }
    }

    /// Loads one snapshot into the subsystem authorities and flushes pending edits.
    pub(crate) fn sync_user_settings(&mut self, runtime: Option<ResMut<RuntimeSettings>>) {
        if self.settings_apply
            && let Some(mut runtime) = runtime
        {
            let mut user = self.settings_options.user_settings();
            // Native window and viewport adapters own these saved preferences.
            user.video.ui_scale = runtime.user_settings_update().1.video.ui_scale;
            user.video.fullscreen = self.fullscreen;
            user.video.render_mode = runtime.user_settings_update().1.video.render_mode;
            runtime.replace_user_settings(user);
            self.settings_apply = false;
        }
        if self.settings_dirty {
            let path = self.config_path.with_file_name(SETTINGS_FILE);
            match self.settings_options.save(&path) {
                Ok(()) => self.settings_dirty = false,
                Err(error) => {
                    self.settings_dirty = false;
                    self.message = Some(format!("Could not save settings: {error}"));
                }
            }
        }
    }

    /// Bridges legacy named menu actions into the persisted option registry.
    pub(in crate::menu) fn set_named_option(&mut self, name: &str, value: i32) {
        if let Some(index) = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
        {
            self.set_option(index as u16, value);
        }
    }
}
