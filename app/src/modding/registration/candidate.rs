//! Candidates are admitted and initialized off the client frame thread.

use super::*;

pub(super) enum Runtime {
    Frame(crate::modding::admission::PreparedMod),
    Packet(Arc<mod_host::packet_mod::CompiledPacketMod>),
}

pub(super) struct Candidate {
    pub(super) runtime: Runtime,
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
    if mod_host::SourceKind::of(&component) == mod_host::SourceKind::Packet
        || registration.grants.packets
    {
        let cache = crate::modding::packets::cache_directory(
            registration
                .component
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        );
        let compiled = mod_host::packet_mod::CompiledPacketMod::compile(
            &component,
            &registration.grants,
            Some(&cache),
        )
        .map_err(|error| format!("{error:#}"))?;
        return Ok(Candidate {
            runtime: Runtime::Packet(Arc::new(compiled)),
            identity,
            registration,
        });
    }
    let prepared = crate::modding::admission::prepare(
        &registration.component,
        &component,
        registration.grants.clone(),
        font_bytes.as_deref(),
        ui.as_deref(),
        settings.map(|settings| (settings.path.as_path(), settings.json.as_str())),
    )?;
    Ok(Candidate {
        runtime: Runtime::Frame(prepared),
        identity,
        registration,
    })
}
