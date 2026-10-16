//! The persisted mod list and lossless migration of the original single selection.

use mod_host::ModGrants;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The canonical list and per-component status protocol version.
pub(super) const VERSION: u32 = 2;

/// One installed component's retained authority and reload request.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(in crate::modding) struct Entry {
    pub component: PathBuf,
    pub enabled: bool,
    #[serde(default)]
    pub font: Option<PathBuf>,
    #[serde(default)]
    pub ui: Option<PathBuf>,
    #[serde(default)]
    pub grants: ModGrants,
    #[serde(default)]
    pub reload: u64,
}

/// Entries retain grants while disabled; enabled entries run in list order.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Registration {
    pub version: u32,
    pub request_id: String,
    pub mods: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Single {
    version: u32,
    request_id: String,
    enabled: bool,
    component: PathBuf,
    #[serde(default)]
    font: Option<PathBuf>,
    #[serde(default)]
    ui: Option<PathBuf>,
    #[serde(default)]
    grants: ModGrants,
}

impl Registration {
    /// Creates an empty list ready for its first Settings command.
    pub(super) fn empty() -> Self {
        Self {
            version: VERSION,
            request_id: "initial".into(),
            mods: Vec::new(),
        }
    }

    /// Decodes either supported format, normalizing the single selection without losing grants.
    pub(super) fn decode(bytes: &[u8]) -> Result<Self, String> {
        let version = serde_json::from_slice::<serde_json::Value>(bytes)
            .map_err(|error| error.to_string())?["version"]
            .as_u64();
        let result = if version == Some(1) {
            let old: Single = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
            debug_assert_eq!(old.version, 1);
            Self {
                version: VERSION,
                request_id: old.request_id,
                mods: vec![Entry {
                    component: old.component,
                    enabled: old.enabled,
                    font: old.font,
                    ui: old.ui,
                    grants: old.grants,
                    reload: 0,
                }],
            }
        } else {
            serde_json::from_slice(bytes).map_err(|error| error.to_string())?
        };
        result.validate()?;
        Ok(result)
    }

    /// Rejects ambiguous identities, unbounded counts and relative source paths.
    pub(super) fn validate(&self) -> Result<(), String> {
        if self.version != VERSION {
            return Err("unsupported local extension registration version".into());
        }
        if self.request_id.is_empty()
            || self.request_id.len() > super::MAX_REQUEST_BYTES
            || self.request_id.chars().any(char::is_control)
        {
            return Err("registration request ID must be bounded plain text".into());
        }
        if self.mods.len() > mod_api::MAX_LOADED_MODS {
            return Err("local extension limit reached".into());
        }
        if self
            .mods
            .iter()
            .filter(|entry| entry.enabled && entry.grants.packets)
            .count()
            > 1
        {
            return Err("Only one packet mod can be enabled at a time.".into());
        }
        for (index, entry) in self.mods.iter().enumerate() {
            if !entry.component.is_absolute()
                || entry.font.as_ref().is_some_and(|path| !path.is_absolute())
                || entry.ui.as_ref().is_some_and(|path| !path.is_absolute())
            {
                return Err("registration component, font and UI paths must be absolute".into());
            }
            entry.grants.validate().map_err(|error| error.to_string())?;
            if entry.enabled
                && entry.grants.settings
                && self.mods[..index].iter().any(|old| {
                    old.enabled
                        && old.grants.settings
                        && super::selection::same_path(
                            &mod_host::settings_companion(&old.component),
                            &mod_host::settings_companion(&entry.component),
                        )
                })
            {
                return Err("active mods must not share a settings companion".into());
            }
            if self.mods[..index]
                .iter()
                .any(|old| super::selection::same_path(&old.component, &entry.component))
            {
                return Err("duplicate local extension identity".into());
            }
        }
        Ok(())
    }
}
