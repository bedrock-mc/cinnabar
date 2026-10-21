//! Bounded reads of the registration file and the extension sources it names.

use std::path::Path;

use client_extensions::modding::font;
use sha2::{Digest, Sha256};

use super::{Entry, MAX_BYTES, MAX_REQUEST_BYTES, Registration, SourceSnapshot};

pub(super) struct Observation {
    pub(super) fingerprint: [u8; 32],
    pub(super) request_id: Option<String>,
    pub(super) result: Result<Option<Registration>, String>,
}

/// Reads a bounded registration and normalizes directory aliases without following leaf links.
pub(super) fn observe(path: &Path) -> Observation {
    let bytes = (|| {
        let bytes = match mod_host::local_files::read(path, MAX_BYTES) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        Ok(Some(bytes))
    })();
    match bytes {
        Ok(None) => Observation {
            fingerprint: [0; 32],
            request_id: None,
            result: Ok(None),
        },
        Err(error) => Observation {
            fingerprint: Sha256::digest(error.as_bytes()).into(),
            request_id: None,
            result: Err(format!("registration could not be read: {error}")),
        },
        Ok(Some(bytes)) => {
            let request_id = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|value| {
                    value
                        .get("request_id")
                        .and_then(|id| id.as_str())
                        .map(str::to_owned)
                })
                .filter(|id| id.len() <= MAX_REQUEST_BYTES && !id.chars().any(char::is_control));
            let result = if bytes.len() > MAX_BYTES {
                Err("local extension registration exceeds byte limit".into())
            } else {
                Registration::decode(&bytes)
                    .map_err(|error| format!("invalid local extension registration: {error}"))
                    .map(|mut registration| {
                        for entry in &mut registration.mods {
                            entry.component =
                                super::selection::resolved_component(&entry.component);
                        }
                        Some(registration)
                    })
            };
            Observation {
                fingerprint: Sha256::digest(&bytes).into(),
                request_id,
                result,
            }
        }
    }
}

/// Captures immutable guest and presentation bytes together with a bounded settings observation.
pub(super) fn source_snapshot(registration: Entry) -> Result<SourceSnapshot, String> {
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(&registration).map_err(|error| error.to_string())?);
    let component = mod_host::read_source(&registration.component)
        .map_err(|error| format!("Mod could not be read: {error}"))?;
    digest.update(Sha256::digest(&component));
    let font = if registration.grants.controls {
        registration
            .font
            .as_deref()
            .map(|path| read_bounded_source(path, font::MAX_SOURCE_BYTES))
            .transpose()?
    } else {
        None
    };
    if let Some(font) = &font {
        digest.update(Sha256::digest(font));
    }
    let ui = if registration.grants.controls {
        registration
            .ui
            .as_deref()
            .map(|path| read_bounded_source(path, ui::mod_panel::MAX_PANEL_BUNDLE_BYTES))
            .transpose()?
    } else {
        None
    };
    if let Some(ui) = &ui {
        digest.update(Sha256::digest(ui));
    }
    let settings = crate::modding::admission::settings_seed(
        registration.grants.settings,
        None,
        &registration.component,
    );
    Ok(SourceSnapshot {
        registration,
        drawing: None,
        identity: digest.finalize().into(),
        component,
        font,
        ui,
        settings,
    })
}

/// Uses the same regular-file and byte-limit admission as component imports.
fn read_bounded_source(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    mod_host::local_files::read(path, max).map_err(|error| {
        format!(
            "extension source {} could not be read: {error}",
            path.display()
        )
    })
}
