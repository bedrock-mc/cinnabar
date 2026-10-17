//! Candidates are admitted and initialized off the client frame thread.

use super::*;

pub(super) struct Candidate {
    pub(super) host: ModHost,
    pub(super) font: Option<Arc<RuntimeFontCatalog>>,
    pub(super) panel_ui: Option<Arc<mod_ui::PreparedPanelUi>>,
    pub(super) branding:
        Option<Arc<client_ui::ui_runtime::presentation::menu_branding::PreparedMenuBranding>>,
    pub(super) identity: [u8; 32],
    pub(super) registration: Entry,
    pub(super) drawing: Option<super::super::admission::DrawingContext>,
}

/// Prepares a fixture using its on-disk preferences and the production admission path.
#[cfg(test)]
pub(super) fn build_candidate(snapshot: SourceSnapshot) -> Result<Candidate, String> {
    build_candidate_with_settings(snapshot, None)
}

/// Admits one snapshot using the last accepted instance's current preferences when available.
pub(super) fn build_candidate_with_settings(
    snapshot: SourceSnapshot,
    settings: Option<&SettingsSnapshot>,
) -> Result<Candidate, String> {
    let SourceSnapshot {
        registration,
        identity,
        component,
        drawing,
        font: font_bytes,
        ui,
    } = snapshot;
    let prepared = crate::modding::admission::prepare(
        &registration.component,
        &component,
        registration.grants.clone(),
        font_bytes.as_deref(),
        ui.as_deref(),
        settings.map(|settings| (settings.path.as_path(), settings.json.as_str())),
        drawing,
    )?;
    Ok(Candidate {
        host: prepared.host,
        font: prepared.font,
        panel_ui: prepared.panel_ui,
        branding: prepared.branding,
        identity,
        registration,
        drawing,
    })
}

/// A rejected source and fence are retried only when an admission input changes.
struct FailedAdmission {
    source: [u8; 32],
    drawing: Option<mod_api::drawing::DrawingSession>,
    settings: Option<SettingsSnapshot>,
    error: String,
}

/// Retains one failed candidate per enabled component without recompiling it on neighbor changes.
#[derive(Default)]
pub(super) struct Admissions {
    failed: BTreeMap<PathBuf, FailedAdmission>,
}

impl Admissions {
    /// Drops failures for identities that no longer belong to this bounded registration.
    pub(super) fn retain(&mut self, registration: &Registration) {
        self.failed.retain(|path, _| {
            registration
                .mods
                .iter()
                .any(|entry| entry.enabled && &entry.component == path)
        });
    }

    /// Prepares a changed source, fence or settings seed once, retaining a refusal for retries.
    pub(super) fn prepare(
        &mut self,
        snapshot: SourceSnapshot,
        settings: Option<&SettingsSnapshot>,
    ) -> Result<Candidate, String> {
        use sha2::{Digest, Sha256};
        let source = snapshot.identity;
        let path = snapshot.registration.component.clone();
        let drawing = snapshot
            .drawing
            .map(|drawing| drawing.policy(Sha256::digest(&snapshot.component).into()));
        if let Some(failed) = self.failed.get(&path)
            && failed.source == source
            && failed.drawing == drawing
            && failed.settings.as_ref() == settings
        {
            return Err(failed.error.clone());
        }
        let result = build_candidate_with_settings(snapshot, settings);
        match &result {
            Ok(_) => {
                self.failed.remove(&path);
            }
            Err(error) => {
                self.failed.insert(
                    path,
                    FailedAdmission {
                        source,
                        drawing,
                        settings: settings.cloned(),
                        error: error.clone(),
                    },
                );
            }
        }
        result
    }
}
