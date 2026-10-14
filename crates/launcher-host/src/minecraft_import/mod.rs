//! Consent-gated import composition, called by the launcher's existing serial pack worker.

mod locations;
mod packs;
mod preview;
mod storage;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result};
use launcher::{
    menu::{SavedServer, settings_options::SettingsOptions},
    minecraft_import::{Action, Phase, Snapshot},
};
use resource_pack::{ActivePack, GlobalPackLibrary, ValidatedPackStack};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// All player preferences replaced by import, also used as the durable restore snapshot.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserState {
    pub settings: SettingsOptions,
    pub servers: Vec<SavedServer>,
    pub video: crate::video_settings::SavedVideoSettings,
    pub active: Vec<ActivePack>,
}

/// A worker request carries current launcher state rather than rereading stale settings files.
#[derive(Debug)]
pub struct Request {
    pub action: Action,
    pub state: UserState,
}

/// A completed operation; runtime publication still follows the normal pack handoff.
pub struct Outcome {
    pub snapshot: Snapshot,
    pub state: Option<UserState>,
    pub stack: Option<Arc<ValidatedPackStack>>,
}

/// Checks only known directory metadata and Cinnabar markers when offering the first prompt.
pub fn startup(config_root: &Path) -> Snapshot {
    startup_at(config_root, &locations::current())
}

/// Keeps detection independent of file payloads and makes consent behavior testable.
fn startup_at(config_root: &Path, locations: &[locations::Location]) -> Snapshot {
    Snapshot {
        phase: if !storage::answered(config_root)
            && locations.iter().any(|location| location.path.is_dir())
        {
            Phase::Offered
        } else {
            Phase::Idle
        },
        can_revert: storage::has_backup(config_root),
        ..Default::default()
    }
}

/// Owns previews on a worker; construction performs no Minecraft reads.
pub struct Service {
    config_root: PathBuf,
    locations: Vec<locations::Location>,
    sources: Vec<locations::Source>,
    preview: Option<preview::Preview>,
    snapshot: Snapshot,
}

impl Service {
    /// Creates an idle worker-side importer without detecting or opening game data.
    pub fn new(config_root: PathBuf) -> Self {
        Self {
            config_root,
            locations: locations::current(),
            sources: Vec::new(),
            preview: None,
            snapshot: Snapshot::default(),
        }
    }

    /// Uses one synthetic data root so integration fixtures never inspect the host installation.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_with_data_root(config_root: PathBuf, data_root: PathBuf) -> Self {
        Self {
            config_root,
            locations: vec![locations::Location {
                path: data_root,
                label: "Fixture".into(),
                profiles: false,
            }],
            sources: Vec::new(),
            preview: None,
            snapshot: Snapshot::default(),
        }
    }

    /// Executes one explicit request in order, returning a visible error on failure.
    pub fn act(&mut self, request: Request, library: &mut GlobalPackLibrary) -> Outcome {
        let previous = library.active().to_vec();
        let result = self.perform(request, library);
        let (state, stack) = match result {
            Ok(result) => result,
            Err(error) => {
                if library.active() != previous {
                    let _ = library.stage_selection(&previous);
                }
                tracing::warn!("Minecraft import failed: {error:#}");
                self.snapshot.phase = Phase::Result;
                self.snapshot.body = format!("Import could not finish: {error:#}.");
                (None, None)
            }
        };
        self.snapshot.can_revert = storage::has_backup(&self.config_root);
        Outcome {
            snapshot: self.snapshot.clone(),
            state,
            stack,
        }
    }

    /// Persists an answer before reading; confirmation installs only a retained preview.
    fn perform(
        &mut self,
        request: Request,
        library: &mut GlobalPackLibrary,
    ) -> Result<(Option<UserState>, Option<Arc<ValidatedPackStack>>)> {
        let Request { action, mut state } = request;
        match action {
            Action::NotNow => {
                storage::answer(&self.config_root, false)?;
                self.preview = None;
                self.snapshot.phase = Phase::Idle;
            }
            Action::Close if self.snapshot.phase == Phase::ConfirmImport => {
                self.snapshot.phase = Phase::Preview;
            }
            Action::Close => {
                self.preview = None;
                self.snapshot.phase = Phase::Idle;
            }
            Action::Begin => {
                storage::answer(&self.config_root, true)?;
                self.preview = None;
                self.sources = locations::discover(&self.locations)?;
                self.snapshot.profiles = self
                    .sources
                    .iter()
                    .map(|source| source.label.clone())
                    .collect();
                self.snapshot.phase = Phase::Profiles;
                self.snapshot.body = if self.sources.is_empty() {
                    "No supported Minecraft user data was found.".to_owned()
                } else {
                    "Choose the profile to preview. Settings and servers come from that profile; shared user packs are included.".to_owned()
                };
                if self.sources.len() == 1 {
                    self.select(0, &state, library)?;
                }
            }
            Action::Select(index) if self.snapshot.phase == Phase::Profiles => {
                self.select(index, &state, library)?
            }
            Action::Apply if self.snapshot.phase == Phase::Preview => {
                self.snapshot.phase = Phase::ConfirmImport;
            }
            Action::ConfirmApply if self.snapshot.phase == Phase::ConfirmImport => {
                let mut preview = self
                    .preview
                    .take()
                    .context("Review the import before applying it")?;
                anyhow::ensure!(
                    preview.catalog == library.available()
                        && preview.original_active == library.active(),
                    "Resource packs changed; preview the import again"
                );
                state.active = library.active().to_vec();
                storage::backup(&self.config_root, &state)?;
                library.retain_for_restore(&state.active)?;
                let mut skipped = preview.skipped;
                for pack in preview.packs {
                    let imported = library.import_prepared(pack)?;
                    for active in &mut preview.selection {
                        if let Some(pack) =
                            imported.imported.iter().find(|pack| pack.id == active.id)
                        {
                            active.revision = pack.revision;
                        }
                    }
                }
                let stack = library.stage_selection(&preview.selection)?;
                skipped += preview.options.apply(&mut state.settings);
                if let Some(fullscreen) = preview.options.fullscreen {
                    state.video.fullscreen = fullscreen;
                }
                let (servers, server_skips) =
                    preview::merge_servers(&state.servers, &preview.servers.entries);
                state.servers = servers;
                skipped += server_skips;
                state.active = library.active().to_vec();
                self.snapshot.phase = Phase::Result;
                self.snapshot.body = format!(
                    "Settings and servers imported; resource packs are being applied. Skipped {skipped} entries. Revert restores the previous preferences and active pack order; copied packs stay installed."
                );
                return Ok((Some(state), Some(stack)));
            }
            Action::Revert => {
                storage::restore(&self.config_root)?;
                self.snapshot.phase = Phase::Revert;
                self.snapshot.body = "Restore the settings, server list and active pack order saved before the last import? This also replaces changes you made since that import. Copied packs stay installed.".to_owned();
            }
            Action::ConfirmRevert if self.snapshot.phase == Phase::Revert => {
                let mut restored = storage::restore(&self.config_root)?;
                let stack = library.stage_selection(&restored.active)?;
                restored.active = library.active().to_vec();
                self.snapshot.phase = Phase::Result;
                self.snapshot.body = "Previous settings and servers restored; previous resource packs are being applied. Copied packs remain installed.".to_owned();
                return Ok((Some(restored), Some(stack)));
            }
            _ => {}
        }
        Ok((None, None))
    }

    /// Replaces the previous preview only after selecting a discovered profile index.
    fn select(
        &mut self,
        index: usize,
        state: &UserState,
        library: &GlobalPackLibrary,
    ) -> Result<()> {
        let source = self
            .sources
            .get(index)
            .context("Choose an available Minecraft profile")?;
        let preview = preview::prepare(source, state, library);
        self.snapshot.phase = Phase::Preview;
        self.snapshot.body = preview.body.clone();
        self.preview = Some(preview);
        Ok(())
    }
}
