//! Bounded component admission and deferred settings activation.

use crate::{MAX_COMPONENT_BYTES, ModGrants, ModHost, runtime::Instance, settings};
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};
use wasmtime::{Config, Engine};

impl ModHost {
    /// Loads a local component with HUD and demo input, denying optional capabilities.
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with_grants(path, ModGrants::default())
    }

    /// Loads a component with the developer's explicit per-mod capability grants.
    pub fn load_with_grants(path: &Path, grants: ModGrants) -> Result<Self> {
        let bytes = read_component(path)?;
        Self::load_snapshot_with_grants(path, &bytes, grants)
    }

    /// Compiles one bounded caller-owned snapshot without rereading its component file.
    pub fn load_snapshot_with_grants(path: &Path, bytes: &[u8], grants: ModGrants) -> Result<Self> {
        let mut host = Self::prepare_snapshot_with_grants(path, bytes, grants, None)?;
        host.activate_settings(None);
        Ok(host)
    }

    /// Prepares a candidate without persisting init output; activation follows publication.
    /// A matching companion snapshot preserves committed preferences ahead of disk writes.
    pub fn prepare_snapshot_with_grants(
        path: &Path,
        bytes: &[u8],
        grants: ModGrants,
        current_settings: Option<(&Path, &str)>,
    ) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_COMPONENT_BYTES,
            "component exceeds byte limit"
        );
        grants.validate()?;
        let mut config = Config::new();
        config.wasm_component_model(true).consume_fuel(true);
        config.max_wasm_stack(256 * 1024);
        let engine = Engine::new(&config)?;
        let settings_writer = if grants.settings {
            Some(settings::SettingsWriter::prepare(path).context("start mod settings writer")?)
        } else {
            None
        };
        let matching_settings = current_settings.filter(|(path, _)| {
            settings_writer
                .as_ref()
                .is_some_and(|writer| writer.destination() == *path)
        });
        let seed = if let Some((_, json)) = matching_settings {
            ensure!(
                json.len() <= mod_api::MAX_SETTINGS_BYTES,
                "mod settings exceed byte limit"
            );
            json.to_owned()
        } else {
            read_settings(path, &grants)?
        };
        let settings_seed = grants.settings.then(|| seed.clone());
        let instance = Instance::new(&engine, bytes, grants.clone(), seed)?;
        Ok(Self {
            engine,
            instance,
            path: path.to_owned(),
            attempted: Sha256::digest(bytes).into(),
            grants,
            settings_writer,
            settings_seed,
        })
    }

    /// Transfers the accepted predecessor's lane so writes remain in commit order.
    /// This performs no file access, thread creation or joins.
    pub fn activate_settings(&mut self, previous: Option<&mut Self>) {
        if let Some(previous) = previous
            && self.settings_path().is_some()
            && self.settings_path() == previous.settings_path()
        {
            std::mem::swap(&mut self.settings_writer, &mut previous.settings_writer);
        }
        if let Some(writer) = self.settings_writer.as_mut() {
            writer.activate();
        }
        self.queue_settings();
    }

    pub fn settings_path(&self) -> Option<&Path> {
        self.settings_writer
            .as_ref()
            .map(settings::SettingsWriter::destination)
    }

    pub fn settings_snapshot(&self) -> Option<&str> {
        self.grants.settings.then(|| self.instance.settings())
    }

    pub fn grants(&self) -> &ModGrants {
        &self.grants
    }

    pub fn settings_seed(&self) -> Option<&str> {
        self.settings_seed.as_deref()
    }
}

impl ModGrants {
    /// Rejects command grants that are not short bare command names.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.commands.len() <= mod_api::MAX_COMMAND_GRANTS
                && self.commands.iter().all(|name| {
                    !name.is_empty()
                        && name.len() <= mod_api::MAX_CONTROL_KEY_BYTES
                        && name.bytes().all(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'
                        })
                }),
            "command grants must be at most {} lowercase command names",
            mod_api::MAX_COMMAND_GRANTS
        );
        Ok(())
    }
}

pub(crate) fn read_settings(path: &Path, grants: &ModGrants) -> Result<String> {
    if !grants.settings {
        return Ok(String::new());
    }
    let path = path.with_extension("settings.json");
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("read mod settings {}", path.display()));
        }
    };
    let mut bytes = Vec::new();
    file.take((mod_api::MAX_SETTINGS_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= mod_api::MAX_SETTINGS_BYTES,
        "mod settings exceed byte limit"
    );
    Ok(String::from_utf8(bytes)?)
}

/// Bounds file reads even if a writer grows the file between metadata and read.
pub(crate) fn read_component(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("open mod {}", path.display()))?
        .take((MAX_COMPONENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_COMPONENT_BYTES,
        "component exceeds byte limit"
    );
    Ok(bytes)
}
