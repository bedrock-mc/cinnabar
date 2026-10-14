//! Persists video settings after their Bevy adapters publish applied values, and hands the
//! host the Video section's requests.
use super::MenuRuntime;
use bevy::prelude::ResMut;
use launcher_host::video_settings::{SavedVideoSettings, save, writer};
use ui::RenderMode;

/// Run after the fullscreen and GUI adapters so the stored tuple is the applied
/// mode plus the raw modifier. A viewport clamp never changes the saved value.
pub fn persist_video_settings(mut menu: ResMut<MenuRuntime>) {
    let settings = SavedVideoSettings {
        fullscreen: menu.fullscreen,
        gui_scale_offset: menu.gui_scale_offset,
    };
    if let Some(writer) = menu.video_settings_writer.as_mut() {
        let results = writer.poll();
        for (saved, result) in results {
            match result {
                Ok(()) => {
                    menu.last_saved_video_settings = saved;
                    menu.failed_video_settings_save = None;
                }
                Err(error) => {
                    menu.failed_video_settings_save = Some(saved);
                    menu.message = Some(format!("Video settings could not be saved: {error}"));
                }
            }
        }
    }
    if (settings == menu.last_saved_video_settings && menu.video_settings_writer.is_none())
        || Some(settings) == menu.failed_video_settings_save
    {
        return;
    }
    if menu.video_settings_writer.is_none() {
        let root = menu.layout.user_config_root.clone();
        match writer::Writer::new(move |value| {
            save(&root, value).map_err(|error| format!("{error:#}"))
        }) {
            Ok(writer) => menu.video_settings_writer = Some(writer),
            Err(error) => {
                menu.message = Some(format!("Video settings writer could not start: {error}"));
                return;
            }
        }
    }
    if let Err(error) = menu
        .video_settings_writer
        .as_mut()
        .expect("writer started")
        .submit(settings)
    {
        menu.failed_video_settings_save = Some(settings);
        menu.message = Some(format!("Video settings could not be saved: {error}"));
    }
}

impl MenuRuntime {
    /// Mirrors the applied mode; a pending menu toggle wins until taken.
    pub fn sync_render_mode(&mut self, applied: RenderMode) {
        if self.render_mode_request.is_none() {
            self.render_mode = applied;
        }
    }

    /// Shows the VSync toggle locked to a launch-flag override.
    #[must_use]
    pub const fn with_vsync_override(mut self, vsync: Option<bool>) -> Self {
        self.vsync_override = vsync;
        self
    }

    /// Consume the pending Video-section change.
    pub fn take_render_mode_request(&mut self) -> Option<RenderMode> {
        self.render_mode_request.take()
    }

    /// True once per press of "Clear distant terrain cache"; the host then clears the
    /// cache off the frame and reports through [`Self::report_distant_terrain_clear`].
    pub fn take_distant_terrain_clear_request(&mut self) -> bool {
        std::mem::take(&mut self.distant_terrain_clear_request)
    }

    /// Shows the outcome of a distant terrain cache clear.
    pub fn report_distant_terrain_clear(&mut self, message: String) {
        self.message = Some(message);
    }
}

#[cfg(test)]
mod tests;
