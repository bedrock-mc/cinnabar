//! Resolves resource halves against selected versions before reserving their identities.

use super::{locations::Source, preview::safe_path};
use anyhow::{Context, Result};
use resource_pack::{GlobalPackLibrary, GlobalSelectionEntry, PreparedImport};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_PACK_CANDIDATES: usize = 256;

/// Prepares only uninstalled identities, preferring the exact version requested by the profile.
pub(super) fn prepare(
    source: &Source,
    selection: &[GlobalSelectionEntry],
    library: &GlobalPackLibrary,
    skipped: &mut usize,
) -> Vec<PreparedImport> {
    let mut packs: Vec<PreparedImport> = Vec::new();
    let mut archive_bytes = 0usize;
    let installed = library
        .available()
        .iter()
        .map(|pack| pack.id)
        .collect::<std::collections::HashSet<_>>();
    let mut chosen: std::collections::HashMap<_, usize> = std::collections::HashMap::new();
    for root in std::iter::once(&source.root).chain(source.shared.iter()) {
        let paths = match pack_paths(root) {
            Ok(paths) => paths,
            Err(error) => {
                tracing::warn!("Minecraft user packs skipped: {error}");
                *skipped += 1;
                continue;
            }
        };
        for path in paths {
            match resource_pack::prepare_import(&path) {
                Ok(pack) => {
                    *skipped += pack.report.skipped_behavior + pack.report.rejected.len();
                    for pack in pack.into_resource_packs() {
                        let candidate = &pack.report.imported[0];
                        if installed.contains(&candidate.id) {
                            *skipped += 1;
                            continue;
                        }
                        let previous = chosen.get(&candidate.id).copied();
                        let previous_bytes = if let Some(index) = previous {
                            let old = &packs[index].report.imported[0];
                            let requested = selection.iter().find(|entry| entry.id == candidate.id);
                            if requested.is_none_or(|entry| {
                                entry.version != candidate.version || old.version == entry.version
                            }) {
                                *skipped += 1;
                                continue;
                            }
                            packs[index].archive_bytes()
                        } else {
                            0
                        };
                        let total = archive_bytes - previous_bytes + pack.archive_bytes();
                        if total > resource_pack::MAX_STACK_ARCHIVE_BYTES {
                            *skipped += 1;
                            continue;
                        }
                        archive_bytes = total;
                        if let Some(index) = previous {
                            packs[index] = pack;
                            *skipped += 1;
                        } else {
                            chosen.insert(candidate.id, packs.len());
                            packs.push(pack);
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!("Minecraft user pack rejected: {error}");
                    *skipped += 1;
                }
            }
        }
    }
    packs
}

/// Lists ordinary children of resource_packs, never visiting cache or game asset roots.
fn pack_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let directory = match safe_path(root, Path::new("resource_packs")) {
        Ok(path) => path,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(Vec::new());
        }
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)
        .context("List user-installed resource packs")?
        .take(MAX_PACK_CANDIDATES + 1)
    {
        let entry = entry?;
        paths.push(entry.path());
    }
    anyhow::ensure!(
        paths.len() <= MAX_PACK_CANDIDATES,
        "User pack folder exceeds the candidate limit"
    );
    paths.sort();
    Ok(paths)
}
