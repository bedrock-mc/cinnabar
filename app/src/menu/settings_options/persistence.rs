//! Bounded settings loading and atomic replacement keep incomplete writes out of the store.

use std::{
    fs,
    io::{self, Read},
    path::Path,
};

use super::{SETTINGS_OPTIONS, SettingsOptions};

const MAX_SETTINGS_BYTES: u64 = 64 * 1024;
pub(crate) const SETTINGS_FILE: &str = "settings.json";

impl SettingsOptions {
    /// Loads known settings and rejects malformed values without losing defaults.
    pub(crate) fn load(path: &Path) -> Self {
        let mut bytes = Vec::new();
        let Ok(file) = fs::File::open(path) else {
            return Self::default();
        };
        if file
            .take(MAX_SETTINGS_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() as u64 > MAX_SETTINGS_BYTES
        {
            return Self::default();
        }
        Self::decode(&bytes).unwrap_or_default()
    }

    /// Parses and validates stored values against the current option registry.
    pub(super) fn decode(bytes: &[u8]) -> Option<Self> {
        let saved: Self = serde_json::from_slice(bytes).ok()?;
        let mut validated = Self::default();
        for (index, option) in SETTINGS_OPTIONS.iter().enumerate() {
            if let Some(value) = saved.values.get(option.name) {
                validated.set(index, *value);
                validated
                    .values
                    .insert(option.name.to_owned(), validated.get(index));
            }
        }
        if let Some(language) = saved.language() {
            validated.set_language(language);
        }
        validated.keys = saved.keys;
        if !validated.stored_bindings_valid() {
            validated.keys.clear();
        }
        Some(validated)
    }

    /// Replaces the file only after a complete JSON document has been written.
    pub(crate) fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        fs::write(&temporary, bytes)?;
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(())
    }
}
