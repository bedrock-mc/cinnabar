//! Local trust decisions, separated from immutable shared bundle bytes.

use crate::{crypto, manifest::Offer};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub const SETTINGS_FILE: &str = "server-experiences.json";
const MAX_SETTINGS_BYTES: u64 = 256 * 1024;
const MAX_DECISIONS: usize = 256;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Once,
    Always,
    Never,
    Cancel,
    Disable,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub audience: String,
    pub server_key: String,
    pub scope_digest: String,
    pub decision: Decision,
    pub highest_revision: u64,
    /// Current consent; inactive pins retain only their rollback history.
    #[serde(default = "active_default")]
    pub active: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub disabled: bool,
    pub media_muted: bool,
    pub media_autoplay: bool,
    pub pins: Vec<Pin>,
}

impl Settings {
    /// Missing settings ask on first use; malformed settings disable extensions.
    pub fn load(path: &Path) -> Self {
        match Self::read(path) {
            Ok(settings) => settings,
            Err(_) => Self {
                disabled: true,
                ..Self::default()
            },
        }
    }

    /// Loads a bounded settings document without logging identities or keys.
    fn read(path: &Path) -> Result<Self> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_SETTINGS_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_SETTINGS_BYTES,
            "trust settings too large"
        );
        let settings: Self = serde_json::from_slice(&bytes)?;
        ensure!(
            settings.pins.len() <= MAX_DECISIONS,
            "too many trust decisions"
        );
        Ok(settings)
    }

    /// Atomically saves only trust scope, never URLs, session secrets or guest data.
    pub fn save(&self, path: &Path) -> Result<()> {
        ensure!(self.pins.len() <= MAX_DECISIONS, "too many trust decisions");
        let bytes = serde_json::to_vec(self)?;
        ensure!(
            bytes.len() as u64 <= MAX_SETTINGS_BYTES,
            "trust settings too large"
        );
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("settings parent missing"))?;
        fs::create_dir_all(parent)?;
        let temporary = parent.join(format!(".experience-settings-{}", crypto::challenge()?));
        let result = (|| -> Result<()> {
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&temporary, path)?;
            Ok(())
        })();
        let _ = fs::remove_file(temporary);
        result
    }

    /// Always pins key and exact scope; never suppresses the whole destination.
    pub fn decision(&self, offer: &Offer) -> Result<Option<Decision>> {
        if self.disabled {
            return Ok(Some(Decision::Never));
        }
        if self.pins.iter().any(|pin| {
            pin.active && pin.audience == offer.audience && pin.decision == Decision::Never
        }) {
            return Ok(Some(Decision::Never));
        }
        let scope = scope_digest(offer)?;
        Ok(self
            .pins
            .iter()
            .find(|pin| {
                pin.active
                    && pin.audience == offer.audience
                    && pin.server_key == offer.server_key
                    && pin.scope_digest == scope
                    && offer.revision >= pin.highest_revision
            })
            .map(|pin| pin.decision))
    }

    /// Records approval with the highest deployment revision seen for this scope.
    pub fn remember(&mut self, offer: &Offer, decision: Decision) -> Result<()> {
        let scope_digest = scope_digest(offer)?;
        let previous = self
            .pins
            .iter()
            .filter(|pin| {
                pin.audience == offer.audience
                    && pin.server_key == offer.server_key
                    && pin.scope_digest == scope_digest
            })
            .map(|pin| pin.highest_revision)
            .max()
            .unwrap_or(0);
        ensure!(
            decision == Decision::Never || offer.revision >= previous,
            "deployment rollback denied"
        );
        let existing = self.pins.iter().position(|pin| {
            pin.audience == offer.audience
                && pin.server_key == offer.server_key
                && pin.scope_digest == scope_digest
        });
        ensure!(
            existing.is_some() || self.pins.len() < MAX_DECISIONS,
            "trust settings are full"
        );
        for pin in self
            .pins
            .iter_mut()
            .filter(|pin| pin.audience == offer.audience)
        {
            pin.active = false;
        }
        let pin = Pin {
            audience: offer.audience.clone(),
            server_key: offer.server_key.clone(),
            scope_digest,
            decision,
            highest_revision: previous.max(offer.revision),
            active: true,
        };
        if let Some(index) = existing {
            self.pins[index] = pin;
        } else {
            self.pins.push(pin);
        }
        Ok(())
    }

    /// Indicates a changed identity so the trusted prompt can explain it.
    pub fn key_changed(&self, offer: &Offer) -> bool {
        self.pins.iter().any(|pin| {
            pin.active && pin.audience == offer.audience && pin.server_key != offer.server_key
        })
    }
}

/// Older settings stored only active decisions, so their pins retain that meaning.
fn active_default() -> bool {
    true
}

/// Includes publishers and package IDs while allowing signed content updates.
fn scope_digest(offer: &Offer) -> Result<String> {
    let publishers: Vec<_> = offer
        .packages
        .iter()
        .map(|p| (&p.id, &p.publisher_key))
        .collect();
    Ok(crypto::digest(&serde_json::to_vec(&(
        &offer.scope,
        publishers,
    ))?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_settings_fail_closed_and_absence_does_not_grant() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SETTINGS_FILE);
        assert!(Settings::load(&path).pins.is_empty());
        std::fs::write(&path, b"broken").unwrap();
        assert!(Settings::load(&path).disabled);
    }
}
