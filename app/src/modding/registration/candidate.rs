//! Candidates are admitted and initialized off the client frame thread.

use super::*;

pub(super) struct Candidate {
    pub(super) host: ModHost,
    pub(super) font: Option<Arc<RuntimeFontCatalog>>,
    pub(super) panel_ui: Option<Arc<mod_ui::PreparedPanelUi>>,
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
    let grants = registration.grants.clone();
    let font = if grants.controls {
        font_bytes
            .as_deref()
            .map(font::rasterize)
            .transpose()?
            .map(Arc::new)
    } else {
        None
    };
    let mut host = ModHost::prepare_snapshot_with_grants(
        &registration.component,
        &component,
        grants,
        settings.map(|settings| (settings.path.as_path(), settings.json.as_str())),
    )
    .map_err(|error| format!("{error:#}"))?;
    if let Some(ui) = ui {
        let json = std::str::from_utf8(&ui).map_err(|error| error.to_string())?;
        host.set_panel_bundle(json)
            .map_err(|error| format!("{error:#}"))?;
    }
    let panel_ui = host
        .panel_bundle()
        .map(mod_ui::PreparedPanelUi::prepare)
        .transpose()?;
    Ok(Candidate {
        host,
        font,
        panel_ui,
        identity,
        registration,
    })
}
