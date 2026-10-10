//! Bounded component and package admission and deferred settings activation.

use crate::{
    DataSource, MAX_COMPONENT_BYTES, ModEvent, ModGrants, ModHost, Source,
    package::{self, Package},
    runtime::{Declared, Instance},
    screens::{declared, loaded},
    settings,
};
use anyhow::{Context, Result, ensure};
use experience_sdk::mod_manifest::{ModManifest, ModPermission};
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path, sync::Arc};
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

    /// Loads the package at `dir` with what its manifest asks for granted, plus `extra`: the
    /// developer profile of a package selected on its own.
    pub fn load_package(dir: &Path, extra: ModGrants) -> Result<Self> {
        let package = Package::read(dir)?;
        let grants = ModGrants {
            screen: true,
            items: true,
            recipes: true,
            keys: true,
            ..extra
        };
        Self::load_read_package(dir, package, grants)
    }

    /// Loads the package at `dir`; each package permission needs both the manifest's ask and
    /// `grants`.
    pub fn load_package_with_grants(dir: &Path, grants: ModGrants) -> Result<Self> {
        let package = Package::read(dir)?;
        Self::load_read_package(dir, package, grants)
    }

    /// Loads a verified package under the loader's original authorization policy.
    fn load_read_package(dir: &Path, package: Package, grants: ModGrants) -> Result<Self> {
        let source = Source::Package(dir.to_owned());
        let authorized_grants = grants;
        let grants = authorized_grants.with_manifest(&package.manifest);
        let declared = declared(&package);
        let mut host = Self::prepare(
            source,
            &package.component,
            grants,
            None,
            declared,
            package.digest,
        )?;
        host.package = Some(loaded(package));
        host.authorized_grants = authorized_grants;
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
        let source = Source::Component(path.to_owned());
        let digest = Sha256::digest(bytes).into();
        Self::prepare(
            source,
            bytes,
            grants,
            current_settings,
            Declared::default(),
            digest,
        )
    }

    fn prepare(
        source: Source,
        bytes: &[u8],
        grants: ModGrants,
        current_settings: Option<(&Path, &str)>,
        declared: Declared,
        digest: [u8; 32],
    ) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_COMPONENT_BYTES,
            "component exceeds byte limit"
        );
        grants.validate()?;
        let path = &source.settings_anchor();
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
        let instance = Instance::new(&engine, bytes, grants.clone(), seed, declared)?;
        Ok(Self {
            engine,
            instance,
            source,
            attempted: digest,
            authorized_grants: grants.clone(),
            grants,
            settings_writer,
            settings_seed,
            package: None,
            layout: None,
            session: Arc::default(),
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

    /// Replaces an instance only after changed bytes compile and initialize. A package is
    /// re-read whole; the new instance gets the current layout and session, and
    /// `screen-changed` and `data-changed` when it has events.
    pub fn reload_if_changed(&mut self) -> Result<bool> {
        let (bytes, digest, package) = match &self.source {
            Source::Component(path) => {
                let bytes = read_component(path)?;
                let digest = Sha256::digest(&bytes).into();
                (bytes, digest, None)
            }
            Source::Package(dir) => {
                let package = Package::read(dir)?;
                (Vec::new(), package.digest, Some(package))
            }
        };
        if self.attempted == digest {
            return Ok(false);
        }
        self.attempted = digest;
        let (bytes, declared) = match &package {
            Some(package) => (&package.component, declared(package)),
            None => (&bytes, Declared::default()),
        };
        let grants = package.as_ref().map_or_else(
            || self.grants.clone(),
            |package| self.authorized_grants.with_manifest(&package.manifest),
        );
        let mut candidate = Instance::new(
            &self.engine,
            bytes,
            grants.clone(),
            self.instance.settings().to_owned(),
            declared,
        )
        .context("reload rejected; previous mod retained")?;
        candidate.set_session(Arc::clone(&self.session));
        self.instance = candidate;
        self.grants = grants;
        if let Some(package) = package {
            self.package = Some(loaded(package));
        }
        self.queue_settings();
        if self.instance.has_events() {
            let layout = ModEvent::ScreenChanged(self.layout.clone());
            let data = ModEvent::DataChanged(vec![DataSource::Items, DataSource::Recipes]);
            self.dispatch(vec![layout, data])?;
        }
        Ok(true)
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

impl Source {
    /// The path whose `.settings.json` companion holds the mod's settings: a component's own
    /// path, or for a package one beside its directory, never inside the hashed package.
    fn settings_anchor(&self) -> std::path::PathBuf {
        match self {
            Self::Component(path) => path.clone(),
            Self::Package(dir) => package::settings_anchor(dir),
        }
    }
}

impl ModGrants {
    /// Applies the manifest's requests without extending the loader's authorization.
    fn with_manifest(&self, manifest: &ModManifest) -> Self {
        let asked = Self::from_manifest(manifest);
        Self {
            screen: asked.screen && self.screen,
            items: asked.items && self.items,
            recipes: asked.recipes && self.recipes,
            keys: asked.keys && self.keys,
            ..self.clone()
        }
    }

    /// What a package's manifest asks for, except `inventory`, which no import carries yet.
    pub fn from_manifest(manifest: &ModManifest) -> Self {
        let asks = |permission| manifest.permissions.contains(&permission);
        Self {
            screen: asks(ModPermission::Screen),
            items: asks(ModPermission::Items),
            recipes: asks(ModPermission::Recipes),
            keys: asks(ModPermission::Keys),
            ..Self::default()
        }
    }

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
