//! Replacement of an opted-in installed component, without changing registration or grants.

use super::installed;
use launcher_host::mod_updates::Downloaded;
use mod_host::{MAX_COMPONENT_SOURCE_BYTES, ModHost, declarations_snapshot, local_files};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Validates the exact download and atomically replaces its unchanged installed source.
/// Existing registration, permission grants and settings companions retain their identities.
pub(super) fn install(mods: &Path, download: &Downloaded) -> Result<String, String> {
    let (item, path) = selected(mods, download)?;
    let old = local_files::read(&path, MAX_COMPONENT_SOURCE_BYTES)
        .map_err(|error| format!("Could not read {}: {error}", item.file))?;
    let bytes = local_files::read(download.path(), MAX_COMPONENT_SOURCE_BYTES)
        .map_err(|error| format!("Could not read mod update: {error}"))?;
    validate(&old, &bytes, download)
        .map_err(|error| format!("Could not update {}: {error}", item.file))?;
    let permissions = fs::symlink_metadata(&path)
        .map_err(|error| error.to_string())?
        .permissions();
    if permissions.readonly() {
        return Err(format!("{} is read-only.", item.file));
    }
    let mut staging = tempfile::NamedTempFile::new_in(mods).map_err(|error| error.to_string())?;
    staging
        .write_all(&bytes)
        .and_then(|()| staging.as_file().sync_all())
        .and_then(|()| staging.as_file().set_permissions(permissions))
        .map_err(|error| format!("Could not stage {}: {error}", item.file))?;
    selected(mods, download)?;
    // Re-read immediately before replacement: an external editor may preserve file metadata.
    let current =
        local_files::read(&path, MAX_COMPONENT_SOURCE_BYTES).map_err(|error| error.to_string())?;
    if current != old {
        return Err("The installed mod changed while its update was prepared.".into());
    }
    if !download.is_current() {
        return Err("Automatic mod update cancelled.".into());
    }
    staging
        .persist(&path)
        .map_err(|error| format!("Could not replace {}: {error}", item.file))?;
    // The existing source watcher reloads changed component bytes and retains the running
    // instance if activation fails; no registration write can grant additional permissions.
    Ok(format!("Updated {} to {}.", item.file, download.version))
}

/// Restricts automatic replacement to the same visible regular file in the managed mods folder.
fn selected(
    mods: &Path,
    download: &Downloaded,
) -> Result<(launcher::mods::InstalledMod, PathBuf), String> {
    if !download.is_current() {
        return Err("Automatic mod update cancelled.".into());
    }
    installed(mods)
        .into_iter()
        .find(|(item, _)| item.id == download.id)
        .ok_or_else(|| "The installed mod changed before its update was ready.".into())
}

/// Requires both snapshots to retain the exact opted-in source and the promised newer version.
fn validate(old: &[u8], bytes: &[u8], download: &Downloaded) -> Result<(), String> {
    let previous = declarations_snapshot(old)
        .map_err(|error| error.to_string())?
        .metadata;
    let next = declarations_snapshot(bytes).map_err(|error| error.to_string())?;
    next.requested
        .map_err(|error| format!("invalid requested permissions: {error}"))?;
    let matches_source = |metadata: &ui::mod_metadata::ModMetadata| {
        metadata.update_url.as_ref().is_some_and(|source| {
            source.repository().slug() == download.repository && source.asset() == download.asset
        })
    };
    if !matches_source(&previous) || !matches_source(&next.metadata) {
        return Err("The update source does not match the installed mod.".into());
    }
    let current = previous
        .version
        .as_deref()
        .ok_or("The installed mod has no version.")?;
    let next_version = next
        .metadata
        .version
        .as_deref()
        .ok_or("The update has no version.")?;
    let normalize = |version: &str| version.strip_prefix('v').unwrap_or(version).to_owned();
    if normalize(next_version) != normalize(&download.version)
        || !update_manifest::select::newer(next_version, current)
            .map_err(|error| error.to_string())?
    {
        return Err("The downloaded version does not match a newer release.".into());
    }
    ModHost::validate_component(bytes).map_err(|error| format!("invalid component: {error:#}"))
}

#[cfg(test)]
mod tests;
