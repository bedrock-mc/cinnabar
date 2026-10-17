//! Read outcomes that distinguish failed candidates without initializing their guests again.

use std::path::Path;

use sha2::{Digest, Sha256};

use super::{Request, font};

type Read = Result<Option<Vec<u8>>, String>;

pub(super) struct Inputs {
    pub(super) font: Read,
    pub(super) ui: Read,
    pub(super) settings: super::admission::SettingsSeed,
}

impl Inputs {
    /// Captures override bytes and the exact preference input before guest initialization.
    pub(super) fn read(request: &Request) -> Self {
        let controls = request.source.grants.controls;
        Self {
            settings: super::admission::settings_seed(
                request.source.grants.settings,
                request.settings.as_ref(),
                &request.source.location.settings_anchor(),
            ),
            font: read(
                request.source.font.as_deref().filter(|_| controls),
                font::MAX_SOURCE_BYTES,
            ),
            ui: read(
                request.source.ui.as_deref().filter(|_| controls),
                ui::mod_panel::MAX_PANEL_BUNDLE_BYTES,
            ),
        }
    }

    /// Frames source policy and override outcomes without rereading any file.
    pub(super) fn fingerprint(
        &self,
        request: &Request,
        component: [u8; 32],
    ) -> Result<[u8; 32], String> {
        let mut digest = Sha256::new();
        digest.update(b"environment component candidate\0");
        digest.update(component);
        field(
            &mut digest,
            &serde_json::to_vec(&request.source.grants).map_err(|error| error.to_string())?,
        );
        for (path, result) in [
            (&request.source.font, &self.font),
            (&request.source.ui, &self.ui),
        ] {
            let path = path.as_deref().filter(|_| request.source.grants.controls);
            field(
                &mut digest,
                path.map_or(&[], |path| path.as_os_str().as_encoded_bytes()),
            );
            outcome(&mut digest, result);
        }
        Ok(digest.finalize().into())
    }
}

/// Uses the source admission limits and preserves missing/unreadable outcomes for retry identity.
fn read(path: Option<&Path>, max: usize) -> Read {
    path.map(|path| mod_host::local_files::read(path, max).map_err(|error| error.to_string()))
        .transpose()
}

/// Separates absent, readable and failed reads before hashing their variable-length contents.
fn outcome(digest: &mut Sha256, result: &Read) {
    match result {
        Ok(None) => digest.update([0]),
        Ok(Some(bytes)) => {
            digest.update([1]);
            field(digest, bytes);
        }
        Err(error) => {
            digest.update([2]);
            field(digest, error.as_bytes());
        }
    }
}

/// Length-prefixes bytes so adjacent candidate fields cannot alias by concatenation.
fn field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}
