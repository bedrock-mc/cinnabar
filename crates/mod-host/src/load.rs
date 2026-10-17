//! Bounded component and package admission and deferred settings activation.

use crate::{
    DataSource, DrawingRestart, MAX_COMPONENT_SOURCE_BYTES, ModEvent, ModGrants, ModHost, Source,
    component_assets::ComponentAssets,
    execution, local_files,
    package::{self, Package},
    runtime::{Declared, Instance},
    screens::{declared, loaded},
    settings,
};
use anyhow::{Context, Result, ensure};
use experience_sdk::mod_manifest::{ModManifest, ModPermission};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};

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

    /// Rejects oversized bytes, invalid imports and incompatible callbacks without running
    /// guest code.
    pub fn validate_component(bytes: &[u8]) -> Result<()> {
        let assets = ComponentAssets::read(bytes)?;
        let (engine, _ticker) = execution::engine()?;
        Instance::check(&engine, &assets.executable)
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
            // The settings file is the environment opt-in's, or the manifest's ask.
            settings: extra.settings
                || package
                    .manifest
                    .permissions
                    .contains(&ModPermission::Settings),
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
        let mut host = Self::prepare_package_in_session(dir, package, grants, None, None)?;
        host.activate_settings(None);
        Ok(host)
    }

    /// Prepares one verified package under its drawing fence without activating settings writes.
    pub fn prepare_package_in_session(
        dir: &Path,
        package: Package,
        grants: ModGrants,
        current_settings: Option<(&Path, &str)>,
        session: Option<mod_api::drawing::DrawingSession>,
    ) -> Result<Self> {
        let source = Source::Package(dir.to_owned());
        let authorized_grants = grants;
        let grants = authorized_grants.with_manifest(&package.manifest);
        let declared = declared(&package);
        let mut host = Self::prepare(
            source,
            &package.component,
            grants,
            current_settings,
            declared,
            package.digest,
            session,
        )?;
        host.package = Some(loaded(package));
        host.authorized_grants = authorized_grants;
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
        Self::prepare_snapshot_in_session(path, bytes, grants, current_settings, None)
    }

    /// Prepares a candidate with its drawing fence installed before guest initialization.
    pub fn prepare_snapshot_in_session(
        path: &Path,
        bytes: &[u8],
        grants: ModGrants,
        current_settings: Option<(&Path, &str)>,
        session: Option<mod_api::drawing::DrawingSession>,
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
            session,
        )
    }

    /// Initializes one bounded snapshot while retaining the loader authorization ceiling.
    fn prepare(
        source: Source,
        bytes: &[u8],
        grants: ModGrants,
        current_settings: Option<(&Path, &str)>,
        declared: Declared,
        digest: [u8; 32],
        session: Option<mod_api::drawing::DrawingSession>,
    ) -> Result<Self> {
        let authorized_grants = grants.clone();
        let grants = if matches!(source, Source::Component(_)) {
            let request = crate::declarations::declarations_snapshot(bytes)?
                .requested?
                .map(|request| request.drawing_permissions())
                .unwrap_or_default();
            grants.with_drawing_request(request)
        } else {
            grants
        };
        let assets = ComponentAssets::read(bytes)?;
        grants.validate()?;
        let path = &source.settings_anchor();
        let (engine, ticker) = execution::engine()?;
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
        let instance = Instance::new_with_drawing_policy(
            &engine,
            &assets.executable,
            grants.clone(),
            seed,
            declared,
            assets.panel_bundle.filter(|_| grants.controls),
            session,
        )?;
        Ok(Self {
            engine,
            _ticker: ticker,
            instance,
            embedded_font: assets.font,
            embedded_logo: assets.logo,
            embedded_title: assets.title,
            embedded_notices: assets.notices,
            panel_bundle_override: None,
            source,
            attempted: digest,
            admitted_digest: digest,
            authorized_grants,
            grants,
            settings_writer,
            settings_seed,
            pending_events: Vec::new(),
            package: None,
            layout: None,
            session: Arc::default(),
            drawing_session: session,
            drawing_restart: DrawingRestart::None,
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
        if self.is_revoked() && !self.needs_drawing_restart() {
            return Ok(false);
        }
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
        let pending_fence = matches!(self.drawing_restart, DrawingRestart::Pending);
        if self.attempted == digest && !pending_fence {
            return Ok(false);
        }
        if !self.instance.can_reload() {
            return Ok(false);
        }
        self.attempted = digest;
        if self.needs_drawing_restart() {
            self.drawing_restart = DrawingRestart::Attempted;
        }
        let (bytes, declared) = match &package {
            Some(package) => (&package.component, declared(package)),
            None => (&bytes, Declared::default()),
        };
        let grants = package.as_ref().map_or_else(
            || self.authorized_grants.clone(),
            |package| self.authorized_grants.with_manifest(&package.manifest),
        );
        let grants = if package.is_none() {
            let request = crate::declarations::declarations_snapshot(bytes)?
                .requested?
                .map(|request| request.drawing_permissions())
                .unwrap_or_default();
            grants.with_drawing_request(request)
        } else {
            grants
        };
        let assets = ComponentAssets::read(bytes)?;
        let bundle = self
            .panel_bundle_override
            .clone()
            .or(assets.panel_bundle)
            .filter(|_| grants.controls);
        let mut candidate = self
            .instance
            .replacement(
                &self.engine,
                &assets.executable,
                grants.clone(),
                self.instance.settings().to_owned(),
                declared,
                bundle,
            )
            .context("reload rejected; previous mod retained")?;
        candidate.set_session(Arc::clone(&self.session));
        self.instance = candidate;
        self.admitted_digest = digest;
        self.drawing_restart = DrawingRestart::None;
        self.embedded_font = assets.font;
        self.embedded_logo = assets.logo;
        self.embedded_title = assets.title;
        self.embedded_notices = assets.notices;
        self.pending_events.clear();
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

    /// Returns the loader's consent before component requests or session ceilings narrow it.
    pub fn authorized_grants(&self) -> &ModGrants {
        &self.authorized_grants
    }

    pub fn grants(&self) -> &ModGrants {
        self.instance.grants()
    }

    pub fn settings_seed(&self) -> Option<&str> {
        self.settings_seed.as_deref()
    }

    /// Shares this accepted source snapshot's optional font only with an authorized panel.
    pub fn panel_font_source(&self) -> Option<&[u8]> {
        (self.grants.controls && self.instance.is_active())
            .then_some(self.embedded_font.as_deref())
            .flatten()
    }

    /// An active instance's menu logo source requires the explicit environment grant.
    pub fn menu_logo_source(&self) -> Option<&[u8]> {
        (self.grants.environment && self.instance.is_active())
            .then_some(self.embedded_logo.as_deref())
            .flatten()
    }

    /// An active instance's menu heading requires the explicit environment grant.
    pub fn menu_title(&self) -> Option<&str> {
        (self.grants.environment && self.instance.is_active())
            .then_some(self.embedded_title.as_deref())
            .flatten()
    }

    /// Copyright notices belong to the accepted source snapshot and carry no guest authority.
    pub fn notices(&self) -> Option<&str> {
        self.embedded_notices.as_deref()
    }

    /// Identifies either source kind for off-frame snapshot admission.
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// The admitted source identity, including embedded static presentation data.
    pub fn source_digest(&self) -> [u8; 32] {
        self.admitted_digest
    }
}

impl Source {
    /// The path whose `.settings.json` companion holds the mod's settings: a component's own
    /// path, or for a package one beside its directory, never inside the hashed package.
    pub fn settings_anchor(&self) -> std::path::PathBuf {
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
            world_draw: asked.world_draw && self.world_draw,
            entity_observe: asked.entity_observe && self.entity_observe,
            entity_highlight: asked.entity_highlight && self.entity_highlight,
            render_through_walls: asked.render_through_walls && self.render_through_walls,
            render_material: asked.render_material && self.render_material,
            render_post: asked.render_post && self.render_post,

            items: asked.items && self.items,
            recipes: asked.recipes && self.recipes,
            keys: asked.keys && self.keys,
            inventory: asked.inventory && self.inventory,
            ..self.clone()
        }
    }

    /// What a package's manifest asks for.
    pub fn from_manifest(manifest: &ModManifest) -> Self {
        let asks = |permission| manifest.permissions.contains(&permission);
        Self {
            screen: asks(ModPermission::Screen),
            world_draw: asks(ModPermission::WorldDraw),
            entity_observe: asks(ModPermission::EntityObserve),
            entity_highlight: asks(ModPermission::EntityHighlight),
            render_through_walls: asks(ModPermission::RenderThroughWalls),
            render_material: asks(ModPermission::RenderMaterial),
            render_post: asks(ModPermission::RenderPost),

            items: asks(ModPermission::Items),
            recipes: asks(ModPermission::Recipes),
            keys: asks(ModPermission::Keys),
            inventory: asks(ModPermission::Inventory),
            settings: asks(ModPermission::Settings),
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
    let path = crate::settings_companion(path);
    let bytes = match local_files::read(&path, mod_api::MAX_SETTINGS_BYTES) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("read mod settings {}", path.display()));
        }
    };
    Ok(String::from_utf8(bytes)?)
}

/// Bounds file reads even if a writer grows the file between metadata and read.
pub(crate) fn read_component(path: &Path) -> Result<Vec<u8>> {
    local_files::read(path, MAX_COMPONENT_SOURCE_BYTES)
        .with_context(|| format!("read mod {}", path.display()))
}
