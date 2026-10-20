//! Bounded user-data reads and preparation; these run only after explicit consent.

use super::{UserState, locations::Source};
use anyhow::Result;
use launcher::minecraft_import::{options, servers};
use resource_pack::{GlobalPackLibrary, ParsedGlobalSelection, PreparedImport};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_TEXT_BYTES: u64 = 64 * 1024;

/// Exact, validated payloads kept on the worker until the user confirms.
#[derive(Debug)]
pub(super) struct Preview {
    pub catalog: Vec<resource_pack::InstalledPack>,
    pub original_active: Vec<resource_pack::ActivePack>,
    pub options: options::ParsedOptions,
    pub servers: servers::ParsedServers,
    pub packs: Vec<PreparedImport>,
    pub selection: Vec<resource_pack::ActivePack>,
    pub skipped: usize,
    pub body: String,
}

/// Reads just the allowlisted files and user pack directory for one selected profile.
pub(super) fn prepare(source: &Source, state: &UserState, library: &GlobalPackLibrary) -> Preview {
    let mut skipped = 0;
    let mut options = read_optional(&source.root, "options.txt", &mut skipped)
        .map_or_else(options::ParsedOptions::default, |bytes| {
            options::parse(&bytes)
        });
    let mut servers = read_optional(&source.root, servers::FILE_NAME, &mut skipped)
        .map_or_else(servers::ParsedServers::default, |bytes| {
            servers::parse(&bytes)
        });
    let selection = read_optional(&source.root, "global_resource_packs.json", &mut skipped)
        .map_or_else(ParsedGlobalSelection::default, |bytes| {
            resource_pack::parse_global_selection(&bytes)
        });
    let packs = super::packs::prepare(source, &selection.entries, library, &mut skipped);
    let (merged, duplicates) = merge_servers(&state.servers, &servers.entries);
    skipped += options.skipped + servers.skipped + selection.skipped + duplicates;
    let mut checked = state.settings.clone();
    let binding_skips = options.apply(&mut checked);
    skipped += binding_skips;
    if binding_skips != 0 {
        options.bindings.clear();
    }
    let new_servers = &merged[state.servers.len()..];
    servers.entries = new_servers.to_vec();
    let pack_metadata = packs
        .iter()
        .flat_map(|pack| &pack.report.imported)
        .collect::<Vec<_>>();
    let available = library
        .available()
        .iter()
        .cloned()
        .chain(pack_metadata.iter().map(|pack| (**pack).clone()))
        .collect::<Vec<_>>();
    let mut accepted = Vec::new();
    let mut ordered = library.active().to_vec();
    for entry in &selection.entries {
        accepted.push(entry.clone());
        let (candidate, selection_skips) = resource_pack::merge_global_selection(
            library.active(),
            &available,
            &accepted,
            library.engine_version(),
        );
        if selection_skips != 0 || library.preview_selection(&candidate, &packs).is_err() {
            accepted.pop();
            skipped += 1;
        } else {
            ordered = candidate;
        }
    }
    let active_names = ordered
        .iter()
        .filter_map(|active| {
            available
                .iter()
                .find(|pack| pack.id == active.id)
                .map(|pack| pack.name.as_str())
        })
        .collect::<Vec<_>>();
    let body = format!(
        "{}\n\nSettings ({})\n{}\n\nNew servers ({})\n{}\n\nResource packs ({})\n{}\n\nActive packs, highest priority first ({})\n{}\n\nSkipped: {skipped} unsupported, duplicate or unavailable entries.\n\nRevert later in Settings; copied packs stay installed.",
        source.label,
        options.count(),
        options.summary(),
        new_servers.len(),
        names(new_servers.iter().map(|server| server.name.as_str())),
        pack_metadata.len(),
        names(pack_metadata.iter().map(|pack| pack.name.as_str())),
        active_names.len(),
        names(active_names.into_iter())
    );
    Preview {
        catalog: library.available().to_vec(),
        original_active: library.active().to_vec(),
        options,
        servers,
        packs,
        selection: ordered,
        skipped,
        body,
    }
}

/// Merges endpoints while preserving every distinct server.
pub(super) fn merge_servers(
    existing: &[launcher::menu::SavedServer],
    incoming: &[launcher::menu::SavedServer],
) -> (Vec<launcher::menu::SavedServer>, usize) {
    let mut merged = existing.to_vec();
    let skipped = servers::merge(&mut merged, incoming);
    (merged, skipped)
}

/// Limits each name and the displayed list while retaining counts for the whole preview.
fn names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let names = names
        .map(|name| {
            name.chars()
                .filter(|c| !c.is_control())
                .take(80)
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    if names.is_empty() {
        return "None".to_owned();
    }
    let mut text = names
        .iter()
        .take(64)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > 64 {
        text.push_str(&format!("; and {} more", names.len() - 64));
    }
    text
}

/// Tries only the two supported user-data locations for an allowlisted filename.
fn read_optional(root: &Path, name: &str, skipped: &mut usize) -> Option<Vec<u8>> {
    for relative in [PathBuf::from("minecraftpe").join(name), PathBuf::from(name)] {
        match read(root, &relative) {
            Ok(bytes) => return Some(bytes),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) => {}
            Err(error) => {
                tracing::warn!("Minecraft user file skipped: {name}: {error}");
                *skipped += 1;
                return None;
            }
        }
    }
    None
}

/// Rejects links in every component beneath the selected root before opening a file.
pub(super) fn safe_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut path = root.to_owned();
    anyhow::ensure!(
        !fs::symlink_metadata(&path)?.file_type().is_symlink(),
        "Linked user-data roots are unsupported"
    );
    for component in relative.components() {
        anyhow::ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "Invalid user-data path"
        );
        path.push(component);
        anyhow::ensure!(
            !fs::symlink_metadata(&path)?.file_type().is_symlink(),
            "Linked user-data files are unsupported"
        );
    }
    Ok(path)
}

/// Reads complete server lists and bounded settings text, rejecting non-regular files.
fn read(root: &Path, relative: &Path) -> Result<Vec<u8>> {
    let path = safe_path(root, relative)?;
    anyhow::ensure!(
        fs::metadata(&path)?.is_file(),
        "User-data source is not a regular file"
    );
    if relative
        .file_name()
        .is_some_and(|name| name == servers::FILE_NAME)
    {
        return Ok(fs::read(path)?);
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_TEXT_BYTES + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_TEXT_BYTES,
        "User-data file exceeds its size limit"
    );
    Ok(bytes)
}
