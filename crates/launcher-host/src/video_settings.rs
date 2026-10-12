//! Durable desktop GUI modifier and fullscreen preferences. The saved modifier
//! is independent of the current viewport and any temporary CLI capture scale.

use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    io::{Read, Write},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[cfg_attr(target_arch = "wasm32", path = "video_settings/writer_web.rs")]
pub mod writer;

/// Persisted video preference file within the user configuration directory.
pub const FILE_NAME: &str = "video-settings.json";
/// Largest video preference file accepted by the reader.
pub const MAX_FILE_BYTES: u64 = 4096;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedVideoSettings {
    pub fullscreen: bool,
    pub gui_scale_offset: i8,
}

/// Reads saved menu and setup geometry without changing the persisted preference.
pub fn load(config_root: &Path) -> Result<SavedVideoSettings> {
    let path = config_root.join(FILE_NAME);
    let Some(bytes) = read_file(&path)? else {
        return Ok(SavedVideoSettings::default());
    };
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail!("{} exceeds the video settings size limit", path.display());
    }
    serde_json::from_slice(&bytes).with_context(|| format!("decode {}", path.display()))
}

/// The saved file's bytes, at most one past [`MAX_FILE_BYTES`], or `None` when there is none.
#[cfg(not(target_arch = "wasm32"))]
fn read_file(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("open {}", path.display())),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("read {}", path.display()))?;
    Ok(Some(bytes))
}

#[cfg(target_arch = "wasm32")]
fn read_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match launcher::browser_files::read(path, MAX_FILE_BYTES + 1) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {}", path.display())),
    }
}

/// Publishes the applied video preferences in the user configuration directory.
pub fn save(config_root: &Path, settings: SavedVideoSettings) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(&settings).context("encode video settings")?;
    replace_file(config_root, &bytes)
}

/// Replaces the browser's stored preferences in one write.
#[cfg(target_arch = "wasm32")]
fn replace_file(config_root: &Path, bytes: &[u8]) -> Result<()> {
    let path = config_root.join(FILE_NAME);
    launcher::browser_files::write(&path, bytes)
        .with_context(|| format!("write {}", path.display()))
}

/// Writes a sibling, then renames it over the saved file, so a reader never sees half of it.
#[cfg(not(target_arch = "wasm32"))]
fn replace_file(config_root: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(config_root).with_context(|| format!("create {}", config_root.display()))?;
    let path = config_root.join(FILE_NAME);
    let temp = config_root.join(format!("{FILE_NAME}.tmp-{}", std::process::id()));
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
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .with_context(|| format!("write {}", temp.display()))?;
        fs::rename(&temp, &path).with_context(|| format!("publish {}", path.display()))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
