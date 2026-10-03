//! Durable desktop GUI modifier and fullscreen preferences. The saved modifier
//! is independent of the current viewport and any temporary CLI capture scale.

use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, Result, bail};
use bevy::prelude::ResMut;
use serde::{Deserialize, Serialize};

use super::MenuRuntime;
pub(super) mod writer;

const FILE_NAME: &str = "video-settings.json";
const MAX_FILE_BYTES: u64 = 4096;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct SavedVideoSettings {
    pub(super) fullscreen: bool,
    pub(super) gui_scale_offset: i8,
}

pub(super) fn load(config_root: &Path) -> Result<SavedVideoSettings> {
    let path = config_root.join(FILE_NAME);
    let file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(SavedVideoSettings::default());
        }
        Err(error) => return Err(error).with_context(|| format!("open {}", path.display())),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {}", path.display()))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail!("{} exceeds the video settings size limit", path.display());
    }
    serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
}

fn save(config_root: &Path, settings: SavedVideoSettings) -> Result<()> {
    fs::create_dir_all(config_root).with_context(|| format!("create {}", config_root.display()))?;
    let path = config_root.join(FILE_NAME);
    let temp = config_root.join(format!("{FILE_NAME}.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(&settings).context("encode video settings")?;
    let _ = fs::remove_file(&temp);
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temp)
            .with_context(|| format!("create {}", temp.display()))?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .with_context(|| format!("write {}", temp.display()))?;
        fs::rename(&temp, &path).with_context(|| format!("publish {}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

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
