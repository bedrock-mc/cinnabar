//! Settings-driven registration writes, and the registration and status the Mods tab shows.

use super::{
    Entry, REGISTRATION_FILE, Registration, STATUS_FILE, StatusList, source::observe,
    status::write_json,
};
use launcher::mods::Status;
use mod_host::ModGrants;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub(super) const SELECTION_CHANGED: &str = "The mod selection changed before removal finished.";

/// The exact registration published for a removal, checked again before deleting files.
#[derive(Clone, Debug)]
pub(in crate::modding) struct Receipt {
    pub(super) request_id: String,
    fingerprint: [u8; 32],
}

impl Receipt {
    /// Rejects a changed or unreadable registration, including changes which reuse its request ID.
    pub(in crate::modding) fn confirm(&self, directory: &Path) -> Result<(), String> {
        let current = observe(&directory.join(REGISTRATION_FILE));
        if current.fingerprint == self.fingerprint && current.result.is_ok() {
            Ok(())
        } else {
            Err(SELECTION_CHANGED.into())
        }
    }
}

/// Matches resolved entries, also resolving a missing companion through its existing parent.
pub(in crate::modding) fn same_path(left: &Path, right: &Path) -> bool {
    left == right
        || resolved_path(left)
            .zip(resolved_path(right))
            .is_some_and(|(left, right)| left == right)
}

/// Resolves directory aliases without requiring a not-yet-created settings entry to exist.
pub(super) fn resolved_path(path: &Path) -> Option<PathBuf> {
    path.canonicalize()
        .ok()
        .or_else(|| Some(path.parent()?.canonicalize().ok()?.join(path.file_name()?)))
}

/// Resolves only directory aliases; a selected leaf link must still fail source admission.
pub(super) fn resolved_component(path: &Path) -> PathBuf {
    path.parent()
        .and_then(|parent| parent.canonicalize().ok())
        .zip(path.file_name())
        .map(|(parent, name)| parent.join(name))
        .unwrap_or_else(|| path.into())
}

/// Uses a resolved component path for panel ownership and cards without a source revision.
pub(in crate::modding) fn component_id(component: &Path) -> u64 {
    let digest = Sha256::digest(component.to_string_lossy().as_bytes());
    u64::from_le_bytes(digest[..8].try_into().expect("eight digest bytes"))
}

/// A valid entry and the status for its list's current request.
pub(in crate::modding) struct Registered {
    pub(in crate::modding) entry: Entry,
    pub(in crate::modding) status: Status,
}

/// Reads the same bounded, migrated registration the watcher consumes.
pub(in crate::modding) fn registered(directory: &Path) -> Vec<Registered> {
    let Ok(Some(registration)) = observe(&directory.join(REGISTRATION_FILE)).result else {
        return Vec::new();
    };
    let statuses = mod_host::local_files::read(&directory.join(STATUS_FILE), super::MAX_BYTES)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StatusList>(&bytes).ok())
        .filter(|status| {
            status.version == super::VERSION && status.request_id == registration.request_id
        });
    registration
        .mods
        .into_iter()
        .map(|entry| {
            let recorded = statuses.as_ref().and_then(|statuses| {
                statuses.mods.iter().find(|status| {
                    same_path(&status.component, &entry.component)
                        && status.reload == Some(entry.reload)
                })
            });
            let status = match recorded {
                Some(recorded) if recorded.state == "loaded" => Status::Running,
                Some(recorded) if recorded.state == "next-join" => Status::NextJoin,
                Some(recorded) if recorded.state == "reload-error" => {
                    Status::ReloadFailed(recorded.message.clone().unwrap_or_default())
                }
                Some(recorded) if recorded.state == "error" => {
                    Status::Failed(recorded.message.clone().unwrap_or_default())
                }
                _ => Status::Starting,
            };
            Registered { entry, status }
        })
        .collect()
}

/// Mutates one list on the library worker, preserving unrelated entries and their grants.
fn edit(
    directory: &Path,
    request_id: String,
    change: impl FnOnce(&mut Registration) -> Result<(), String>,
) -> Result<Receipt, String> {
    let path = directory.join(REGISTRATION_FILE);
    let mut registration = observe(&path).result?.unwrap_or_else(Registration::empty);
    change(&mut registration)?;
    registration.request_id = request_id;
    write_selection(&path, registration)
}

/// Adds a component to Active without replacing another; re-enable retains its authority.
pub(in crate::modding) fn select(
    directory: &Path,
    component: &Path,
    enabled: bool,
    request_id: String,
) -> Result<(), String> {
    edit(directory, request_id, |registration| {
        if let Some(entry) = registration
            .mods
            .iter_mut()
            .find(|entry| same_path(&entry.component, component))
        {
            entry.enabled = enabled;
        } else {
            registration.mods.push(Entry {
                component: resolved_component(component),
                enabled,
                ..Default::default()
            });
        }
        Ok(())
    })
    .map(|_| ())
}

/// Disables exactly the currently active card, rejecting an obsolete selection.
pub(in crate::modding) fn disable(
    directory: &Path,
    component: &Path,
    request_id: String,
) -> Result<(), String> {
    update_entry(directory, component, request_id, |entry| {
        entry.enabled = false;
        Ok(())
    })
}

/// Applies an active-card command to its own component only.
fn update_entry(
    directory: &Path,
    component: &Path,
    request_id: String,
    change: impl FnOnce(&mut Entry) -> Result<(), String>,
) -> Result<(), String> {
    edit(directory, request_id, |registration| {
        let entry = registration
            .mods
            .iter_mut()
            .find(|entry| entry.enabled && same_path(&entry.component, component))
            .ok_or_else(|| "That mod is no longer enabled.".to_owned())?;
        change(entry)
    })
    .map(|_| ())
}

/// Requests a fresh instance even when the component bytes have not changed.
pub(in crate::modding) fn reload(
    directory: &Path,
    component: &Path,
    request_id: String,
) -> Result<(), String> {
    update_entry(directory, component, request_id, |entry| {
        entry.reload = entry
            .reload
            .checked_add(1)
            .ok_or("Reload counter exhausted.")?;
        Ok(())
    })
}

/// Requests fresh instances for all enabled entries using the same admission path.
pub(in crate::modding) fn reload_all(directory: &Path, request_id: String) -> Result<(), String> {
    edit(directory, request_id, |registration| {
        for entry in registration.mods.iter_mut().filter(|entry| entry.enabled) {
            entry.reload = entry
                .reload
                .checked_add(1)
                .ok_or("Reload counter exhausted.")?;
        }
        Ok(())
    })
    .map(|_| ())
}

/// Moves a card among the enabled entries without altering any entry's runtime identity.
pub(in crate::modding) fn move_entry(
    directory: &Path,
    component: &Path,
    up: bool,
    request_id: String,
) -> Result<(), String> {
    edit(directory, request_id, |registration| {
        let active: Vec<_> = registration
            .mods
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.enabled)
            .map(|(index, _)| index)
            .collect();
        let position = active
            .iter()
            .position(|index| same_path(&registration.mods[*index].component, component))
            .ok_or("That mod is no longer enabled.")?;
        let neighbor = if up {
            position.checked_sub(1)
        } else {
            (position + 1 < active.len()).then_some(position + 1)
        }
        .ok_or("That mod cannot move further.")?;
        registration.mods.swap(active[position], active[neighbor]);
        Ok(())
    })
    .map(|_| ())
}

/// Updates one capability generated from the grant declaration, preserving every other mod.
pub(in crate::modding) fn permission(
    directory: &Path,
    component: &Path,
    capability: usize,
    enabled: bool,
    request_id: String,
) -> Result<(), String> {
    change_grants(directory, component, request_id, |grants| {
        let descriptor = ModGrants::CAPABILITIES
            .get(capability)
            .ok_or("Unknown capability.")?;
        let requested = if enabled {
            mod_host::declarations(component)
                .and_then(|declared| declared.requested)
                .map_err(|error| error.to_string())?
        } else {
            None
        };
        (descriptor.set)(grants, enabled, requested.as_ref());
        if enabled && !(descriptor.enabled)(grants) {
            return Err("This component does not declare requested command names.".into());
        }
        Ok(())
    })
}

/// Grants exactly the component's declared requests in one registration write.
pub(in crate::modding) fn allow_requested(
    directory: &Path,
    component: &Path,
    request_id: String,
) -> Result<(), String> {
    let requested = mod_host::declarations(component)
        .and_then(|declared| declared.requested)
        .map_err(|error| error.to_string())?
        .ok_or("This mod doesn't declare its permissions.")?;
    change_grants(directory, component, request_id, |grants| {
        *grants = requested;
        Ok(())
    })
}

/// Clears every capability in one write, even when the source cannot be read.
pub(in crate::modding) fn revoke_all(
    directory: &Path,
    component: &Path,
    request_id: String,
) -> Result<(), String> {
    change_grants(directory, component, request_id, |grants| {
        *grants = ModGrants::default();
        Ok(())
    })
}

/// Applies a permission edit to the component's retained grants and persists it once.
fn change_grants(
    directory: &Path,
    component: &Path,
    request_id: String,
    change: impl FnOnce(&mut ModGrants) -> Result<(), String>,
) -> Result<(), String> {
    edit(directory, request_id, |registration| {
        let position = registration
            .mods
            .iter()
            .position(|entry| same_path(&entry.component, component));
        let position = position.unwrap_or_else(|| {
            registration.mods.push(Entry {
                component: resolved_component(component),
                ..Default::default()
            });
            registration.mods.len() - 1
        });
        change(&mut registration.mods[position].grants)
    })
    .map(|_| ())
}

/// Removes all retained grants and metadata for this identity before its deletion fence.
pub(in crate::modding) fn forget(
    directory: &Path,
    component: &Path,
    request_id: String,
) -> Result<Receipt, String> {
    edit(directory, request_id, |registration| {
        registration
            .mods
            .retain(|entry| !same_path(&entry.component, component));
        Ok(())
    })
}

/// Publishes the canonical validated format and a receipt of its exact bytes.
fn write_selection(path: &Path, registration: Registration) -> Result<Receipt, String> {
    registration.validate()?;
    let bytes = serde_json::to_vec(&registration).map_err(|error| error.to_string())?;
    let fingerprint = Sha256::digest(&bytes).into();
    write_json(path, &registration)
        .map_err(|error| format!("Could not save the mod selection: {error}"))?;
    Ok(Receipt {
        request_id: registration.request_id,
        fingerprint,
    })
}

#[cfg(test)]
mod tests;
