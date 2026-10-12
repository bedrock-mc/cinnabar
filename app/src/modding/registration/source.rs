//! Bounded reads of the registration file and the extension sources it names.

use std::{fs::File, io::Read, path::Path};

use client_extensions::modding::font;
use sha2::{Digest, Sha256};

use super::{MAX_BYTES, MAX_REQUEST_BYTES, Registration, SourceSnapshot};

pub(super) struct Observation {
    pub(super) fingerprint: [u8; 32],
    pub(super) request_id: Option<String>,
    pub(super) result: Result<Option<Registration>, String>,
}

pub(super) fn observe(path: &Path) -> Observation {
    let bytes = (|| {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.to_string()),
        };
        let mut bytes = Vec::new();
        file.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| error.to_string())?;
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
                serde_json::from_slice::<Registration>(&bytes)
                    .map_err(|error| format!("invalid local extension registration: {error}"))
                    .and_then(|registration| registration.validate().map(|()| Some(registration)))
            };
            Observation {
                fingerprint: Sha256::digest(&bytes).into(),
                request_id,
                result,
            }
        }
    }
}

pub(super) fn source_snapshot(registration: Registration) -> Result<SourceSnapshot, String> {
    let mut digest = Sha256::new();
    digest.update(serde_json::to_vec(&registration).map_err(|error| error.to_string())?);
    let component = read_bounded_source(&registration.component, mod_host::MAX_COMPONENT_BYTES)?;
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
    Ok(SourceSnapshot {
        registration,
        identity: digest.finalize().into(),
        component,
        font,
    })
}

fn read_bounded_source(path: &Path, max: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take((max + 1) as u64).read_to_end(&mut bytes))
        .map_err(|error| {
            format!(
                "extension source {} could not be read: {error}",
                path.display()
            )
        })?;
    if bytes.len() > max {
        return Err("extension source exceeds byte limit".into());
    }
    Ok(bytes)
}
