//! Durable desktop GUI modifier and fullscreen preferences. The saved modifier
//! is independent of the current viewport and any temporary CLI capture scale.

use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

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

/// Atomically publishes the applied video preferences in the user configuration directory.
pub fn save(config_root: &Path, settings: SavedVideoSettings) -> Result<()> {
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
