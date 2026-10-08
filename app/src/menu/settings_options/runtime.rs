//! One handoff applies menu edits to the camera, input, window and sound authorities.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use bevy::prelude::ResMut;

use super::{SETTINGS_OPTIONS, persistence::SETTINGS_FILE};
use crate::{menu::MenuRuntime, settings_runtime::RuntimeSettings};

/// Limits disk retries after a failed write without discarding pending preferences.
const SETTINGS_RETRY: Duration = Duration::from_secs(1);

impl MenuRuntime {
    /// Persists slot equipment without republishing unrelated video/input preferences.
    pub(crate) fn set_emote_slot_preferences(
        &mut self,
        slots: [Option<String>; super::EMOTE_SLOT_COUNT],
    ) {
        if Arc::make_mut(&mut self.settings_options).set_emote_slots(slots) {
            self.settings_dirty = true;
        }
    }
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
        // A real edit replaces any session override and is saved.
        self.session_overrides
            .retain(|(overridden, _)| *overridden != usize::from(index));
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
            user.video.enhanced_quality = runtime.user_settings_update().1.video.enhanced_quality;
            runtime.replace_user_settings(user);
            self.settings_apply = false;
        }
        if self.settings_dirty
            && self
                .settings_retry_at
                .is_none_or(|due| Instant::now() >= due)
        {
            let path = self.config_path.with_file_name(SETTINGS_FILE);
            let saved = if self.session_overrides.is_empty() {
                Arc::clone(&self.settings_options)
            } else {
                let mut persisted = (*self.settings_options).clone();
                for &(index, value) in &self.session_overrides {
                    persisted.set(index, value);
                }
                Arc::new(persisted)
            };
            match saved.save(&path) {
                Ok(()) => {
                    self.settings_dirty = false;
                    self.settings_retry_at = None;
                }
                Err(error) => {
                    self.settings_retry_at = Some(Instant::now() + SETTINGS_RETRY);
                    self.message = Some(format!("Could not save settings: {error}"));
                }
            }
        }
    }

    /// Bridges legacy named menu actions into the persisted option registry.
    pub(crate) fn set_named_option(&mut self, name: &str, value: i32) {
        if let Some(index) = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
        {
            self.set_option(index as u16, value);
        }
    }

    /// Overrides `name` in memory only (saves keep the persisted value); `None` restores it.
    pub(crate) fn set_session_option(&mut self, name: &str, value: Option<i32>) {
        let Some(index) = SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)
        else {
            return;
        };
        let slot = self
            .session_overrides
            .iter()
            .position(|(overridden, _)| *overridden == index);
        let value = match (value, slot) {
            (Some(value), Some(_)) => value,
            (Some(value), None) => {
                let persisted = self.settings_options.get(index);
                self.session_overrides.push((index, persisted));
                value
            }
            (None, Some(slot)) => self.session_overrides.swap_remove(slot).1,
            (None, None) => return,
        };
        if Arc::make_mut(&mut self.settings_options).set(index, value) {
            self.settings_apply = true;
        }
    }

    /// Starts or ends a developer-driven stretch; ending restores the hotkey options it toggled.
    #[cfg_attr(
        not(feature = "developer-control"),
        allow(dead_code, reason = "called by the developer control endpoint")
    )]
    pub(crate) fn set_transient_toggles(&mut self, transient: bool) {
        self.transient_toggles = transient;
        if !transient {
            for (_, option) in crate::menu::input::HOTKEY_OPTIONS {
                self.set_session_option(option, None);
            }
        }
    }
}
