//! Candidates are admitted and initialized off the client frame thread.

use super::*;

pub(super) enum Runtime {
    Frame(Box<crate::modding::admission::PreparedMod>),
    Packet(Arc<mod_host::packet_mod::CompiledPacketMod>),
}

pub(super) struct Candidate {
    pub(super) runtime: Runtime,
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
    let settings = snapshot.settings_seed(settings)?;
    let SourceSnapshot {
        registration,
        identity,
        component,
        drawing,
        font: font_bytes,
        ui,
        settings: _,
    } = snapshot;
    if mod_host::SourceKind::of(&component) == mod_host::SourceKind::Packet
        || registration.grants.packets
    {
        let compiled =
            mod_host::packet_mod::CompiledPacketMod::compile(&component, &registration.grants)
                .map_err(|error| format!("{error:#}"))?;
        return Ok(Candidate {
            runtime: Runtime::Packet(Arc::new(compiled)),
            identity,
            registration,
            drawing: None,
        });
    }
    let prepared = crate::modding::admission::prepare(
        &registration.component,
        &component,
        registration.grants.clone(),
        font_bytes.as_deref(),
        ui.as_deref(),
        settings
            .as_ref()
            .map(|settings| (settings.path.as_path(), settings.json.as_str())),
        drawing.clone(),
    )?;
    Ok(Candidate {
        runtime: Runtime::Frame(Box::new(prepared)),
        identity,
        registration,
        drawing,
    })
}

/// A rejected source and fence are retried only when an admission input changes.
struct FailedAdmission {
    source: [u8; 32],
    drawing: Option<mod_api::drawing::DrawingSession>,
    settings: SettingsSeed,
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

    /// Prepares one captured preference input and caches only failures of those exact inputs.
    pub(super) fn prepare(
        &mut self,
        mut snapshot: SourceSnapshot,
        settings: Option<&SettingsSnapshot>,
    ) -> Result<Candidate, String> {
        let source = snapshot.identity;
        let path = snapshot.registration.component.clone();
        let drawing = snapshot.drawing_policy();
        let settings = snapshot.settings_seed(settings);
        snapshot.settings = settings.clone();
        if let Some(failed) = self.failed.get(&path)
            && failed.source == source
            && failed.drawing == drawing
            && failed.settings == settings
        {
            return Err(failed.error.clone());
        }
        let result = match &settings {
            Ok(_) => build_candidate_with_settings(snapshot, None),
            Err(error) => Err(error.clone()),
        };
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
                        settings,
                        error: error.clone(),
                    },
                );
            }
        }
        result
    }
}
