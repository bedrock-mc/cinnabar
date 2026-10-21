//! One handoff applies menu edits to the camera, input, window and sound authorities.

use launcher::menu::settings_options::SettingId;
use std::{sync::Arc, time::Instant};

use bevy::prelude::{AppExit, MessageReader, ResMut};

use launcher::menu::settings_options::{
    SettingsOptions,
    persistence::{
        SETTINGS_FILE,
        writer::{RETRY_DELAY, Writer},
    },
};
use {crate::menu::MenuRuntime, client_state::RuntimeSettings};

impl MenuRuntime {
    /// Refreshes untouched render distance from the selected device before settings publication.
    pub fn sync_render_distance_device(&mut self, device: ui::RenderDistanceDevice) {
        if Arc::make_mut(&mut self.settings_options).set_render_distance_device(device) {
            self.settings_apply = true;
        }
    }

    /// Shares the renderer's sample-count intersection with the Video slider and settings handoff.
    pub fn sync_anti_aliasing_support(&mut self, support: ui::AntiAliasingSupport) {
        if self.settings_options.anti_aliasing_support() != support {
            Arc::make_mut(&mut self.settings_options).set_anti_aliasing_support(support);
            self.settings_apply = true;
        }
    }

    /// Persists slot equipment without republishing unrelated video/input preferences.
    pub fn set_emote_slot_preferences(
        &mut self,
        slots: [Option<String>; launcher::menu::settings_options::EMOTE_SLOT_COUNT],
    ) {
        if Arc::make_mut(&mut self.settings_options).set_emote_slots(slots) {
            self.settings_dirty = true;
        }
    }
    /// Publishes normalized glint factors to the shared UI renderer.
    pub fn ui_glint_settings(&self) -> render_scene::UiGlintSettings {
        render_scene::UiGlintSettings {
            strength: self.settings_options.value(SettingId::GlintStrength) as f32 / 100.0,
            speed: self.settings_options.value(SettingId::GlintSpeed) as f32 / 100.0,
        }
    }

    /// Reads the desktop scoping option for the device that produced this frame's turn.
    pub fn spyglass_damping(&self, mode: semantic_input::InputMode) -> f32 {
        let name = match mode {
            semantic_input::InputMode::KeyboardMouse => SettingId::SpyglassMouseDampening,
            semantic_input::InputMode::GamePad => SettingId::SpyglassGamepadDampening,
            semantic_input::InputMode::Touch => return 0.0,
        };
        self.settings_options.value(name) as f32 / 100.0
    }

    /// Applies a validated setting edit and marks its persistence and runtime handoff dirty.
    pub(in crate::menu) fn set_option(&mut self, index: u16, value: i32) {
        // A real edit replaces any session override and is saved.
        let overrides = self.session_overrides.len();
        self.session_overrides
            .retain(|preference| preference.index() != usize::from(index));
        let changed = Arc::make_mut(&mut self.settings_options).set(usize::from(index), value);
        if changed || overrides != self.session_overrides.len() {
            self.settings_dirty = true;
            self.settings_apply = true;
        }
    }

    /// Applies edits to runtime authorities immediately and queues their disk snapshot.
    pub fn sync_user_settings(&mut self, runtime: Option<ResMut<RuntimeSettings>>) {
        if self.external.is_none() {
            launcher_host::lifecycle::update::configure(self.settings_options.update_preferences());
        }
        if self.settings_apply
            && let Some(mut runtime) = runtime
        {
            let mut user = self.settings_options.user_settings();
            // Native window and viewport adapters own these saved preferences.
            user.video.ui_scale = runtime.user_settings_update().1.video.ui_scale;
            user.video.fullscreen = self.fullscreen;
            user.video.render_mode = runtime.user_settings_update().1.video.render_mode;
            user.video.shader_pack = runtime.user_settings_update().1.video.shader_pack.clone();
            runtime.replace_user_settings(user);
            self.settings_apply = false;
        }
        if let Some(Err(error)) = self
            .settings_writer
            .as_mut()
            .and_then(|writer| writer.poll())
        {
            self.message = Some(format!("Could not save settings: {error}"));
        }
        if self.settings_dirty
            && self
                .settings_retry_at
                .is_none_or(|due| Instant::now() >= due)
        {
            match self.queue_user_settings() {
                Ok(()) => self.settings_retry_at = None,
                Err(error) => {
                    self.settings_retry_at = Some(Instant::now() + RETRY_DELAY);
                    self.message = Some(format!("Could not save settings: {error}"));
                }
            }
        }
    }

    /// Transfers the newest persistent snapshot to the sole disk owner.
    fn queue_user_settings(&mut self) -> std::io::Result<()> {
        if self.external.is_some() {
            self.settings_dirty = false;
            return Ok(());
        }
        if self.settings_writer.is_none() {
            self.settings_writer = Some(Writer::new()?);
        }
        let saved = self.persistent_settings();
        self.settings_writer
            .as_mut()
            .expect("settings writer started")
            .submit(self.config_path.with_file_name(SETTINGS_FILE), saved)?;
        self.settings_dirty = false;
        Ok(())
    }

    /// Flushes even edits made after the last frame, waiting only during orderly shutdown.
    pub(in crate::menu) fn flush_user_settings(&mut self) -> std::io::Result<()> {
        if self.settings_dirty {
            self.queue_user_settings()?;
        }
        if let Some(mut writer) = self.settings_writer.take() {
            writer.finish()?;
        }
        Ok(())
    }

    /// Returns preferences for saves and backups without including session-only overrides.
    pub(in crate::menu) fn persistent_settings(&self) -> Arc<SettingsOptions> {
        if self.session_overrides.is_empty() {
            return Arc::clone(&self.settings_options);
        }
        let mut settings = (*self.settings_options).clone();
        for &preference in &self.session_overrides {
            settings.restore_option_preference(preference);
        }
        Arc::new(settings)
    }

    /// Applies a registered setting through the persisted option controller.
    pub fn set_named_option(&mut self, id: SettingId, value: i32) {
        self.set_option(id.index() as u16, value);
    }

    /// Overrides a setting in memory; None restores the saved preference.
    pub fn set_session_option(&mut self, id: SettingId, value: Option<i32>) {
        let index = id.index();
        let slot = self
            .session_overrides
            .iter()
            .position(|preference| preference.index() == index);
        if let Some(value) = value {
            if slot.is_none() {
                let preference = self
                    .settings_options
                    .option_preference(index)
                    .expect("registered session option");
                self.session_overrides.push(preference);
            }
            if Arc::make_mut(&mut self.settings_options).set(index, value) {
                self.settings_apply = true;
            }
        } else if let Some(slot) = slot {
            let preference = self.session_overrides.swap_remove(slot);
            if Arc::make_mut(&mut self.settings_options).restore_option_preference(preference) {
                self.settings_apply = true;
            }
        }
    }

    /// Starts or ends a developer-driven stretch; ending restores the hotkey options it toggled.
    #[cfg_attr(
        not(feature = "developer-control"),
        allow(dead_code, reason = "called by the developer control endpoint")
    )]
    pub fn set_transient_toggles(&mut self, transient: bool) {
        self.transient_toggles = transient;
        if !transient {
            for (_, option) in crate::menu::input::HOTKEY_OPTIONS {
                self.set_session_option(option, None);
            }
        }
    }
}

/// Drains settings before normal exit starts tearing down rendering and other resources.
pub(crate) fn flush_on_exit(mut exits: MessageReader<AppExit>, menu: Option<ResMut<MenuRuntime>>) {
    if exits.read().next().is_some()
        && let Some(mut menu) = menu
        && let Err(error) = menu.flush_user_settings()
    {
        bevy::log::warn!("could not save settings at shutdown: {error}");
    }
}
