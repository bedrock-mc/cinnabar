//! Persists video settings after their Bevy adapters publish applied values.
use super::MenuRuntime;
use bevy::prelude::ResMut;
use launcher_host::video_settings::{SavedVideoSettings, save, writer};

/// Run after the fullscreen and GUI adapters so the stored tuple is the applied
/// mode plus the raw modifier. A viewport clamp never changes the saved value.
pub(crate) fn persist_video_settings(mut menu: ResMut<MenuRuntime>) {
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

#[cfg(test)]
mod tests;
