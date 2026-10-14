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
    )?;
    Ok(Candidate {
        host: prepared.host,
        font: prepared.font,
        panel_ui: prepared.panel_ui,
        branding: prepared.branding,
        identity,
        registration,
    })
}
