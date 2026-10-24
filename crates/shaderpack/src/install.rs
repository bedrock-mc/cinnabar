//! Adds packs the player dropped onto the window to the shader-pack directory, as Iris's
//! shader pack screen does with dropped files.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    discover::{is_zip_name, pack_folder},
    source::zip_holds_pack,
};

/// Suffix of a copy still being written; [`crate::discover_packs`] skips such entries, so a
/// listing taken mid-copy never offers a partial pack.
pub(crate) const STAGING_SUFFIX: &str = ".importing";

/// Serializes imports and removals so they cannot replace a library entry mid-operation.
pub(crate) static LIBRARY_IO: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// What adding dropped files did. Like Iris, files that are not packs are skipped while any
/// dropped file is one, and the first pack that cannot be copied stops the rest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropOutcome {
    /// These packs were copied in, in drop order.
    Added(Vec<String>),
    /// No dropped file is a pack; carries the file's name when exactly one was dropped.
    NoPacks(Option<String>),
    /// The directory already holds an entry with this pack's name; it was left untouched.
    AlreadyExists(String),
    /// Copying this pack failed with `error`; no partial copy remains.
    CopyFailed { name: String, error: String },
}

/// Whether `path` is a pack the shader-pack directory can list and open: a folder holding
/// `shaders/` directly or inside its single sub-folder, or a `.zip` whose root or single
/// top-level folder holds `shaders/`. Iris accepts a `shaders` folder at any depth; packs
/// nested deeper would be listed but fail to open here, so they are refused up front.
fn is_shader_pack(path: &Path) -> bool {
    if path.is_dir() {
        pack_folder(path.to_path_buf()).is_some()
    } else {
        is_zip_name(path) && zip_holds_pack(path)
    }
}

/// Copies each dropped pack in `paths` into `dir` under its own file or folder name,
/// creating `dir` when missing. Existing entries are never overwritten. Each copy is written
/// under a staging name and renamed into place once complete. Blocking file work: run it off
/// the frame.
pub fn add_dropped_packs(paths: &[PathBuf], dir: &Path) -> DropOutcome {
    let _import = LIBRARY_IO.lock().unwrap_or_else(|error| error.into_inner());
    let packs: Vec<_> = paths
        .iter()
        .filter_map(|path| Some((path, path.file_name()?.to_owned())))
        .filter(|(path, _)| is_shader_pack(path))
        .collect();
    if packs.is_empty() {
        let single = match paths {
            [path] => Some(display_name(path.file_name().unwrap_or(path.as_os_str()))),
            _ => None,
        };
        return DropOutcome::NoPacks(single);
    }
    let mut added = Vec::with_capacity(packs.len());
    for (path, file_name) in packs {
        let name = display_name(&file_name);
        let target = dir.join(&file_name);
        if fs::symlink_metadata(&target).is_ok() {
            return DropOutcome::AlreadyExists(name);
        }
        let copied = reject_overlap(path, &target)
            .and_then(|()| fs::create_dir_all(dir))
            .and_then(|()| copy_pack(path, &target));
        if let Err(error) = copied {
            if fs::symlink_metadata(&target).is_ok() {
                return DropOutcome::AlreadyExists(name);
            }
            return DropOutcome::CopyFailed {
                name,
                error: error.to_string(),
            };
        }
        added.push(name);
    }
    DropOutcome::Added(added)
}

/// Formats an OS file name for an import result.
fn display_name(name: &std::ffi::OsStr) -> String {
    name.to_string_lossy().into_owned()
}

/// Resolves existing symlinks and normalizes a destination whose final components may be missing.
fn resolve_destination(path: &Path) -> io::Result<PathBuf> {
    use std::path::Component;
    let mut resolved = PathBuf::new();
    for component in std::path::absolute(path)?.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                resolved.pop();
            }
            component => {
                resolved.push(component.as_os_str());
                match resolved.canonicalize() {
                    Ok(canonical) => resolved = canonical,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    Ok(resolved)
}

/// Rejects directory copies whose source and destination are ancestors of one another.
fn reject_overlap(source: &Path, target: &Path) -> io::Result<()> {
    if source.is_dir() {
        let target = resolve_destination(target)?;
        let folder = pack_folder(source.to_path_buf());
        let shaders = folder.as_ref().map(|folder| folder.join("shaders"));
        for root in std::iter::once(source)
            .chain(folder.as_deref())
            .chain(shaders.as_deref())
        {
            let root = root.canonicalize()?;
            if target.starts_with(&root) || root.starts_with(&target) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "pack source and destination overlap",
                ));
            }
        }
    }
    Ok(())
}

/// Copies into unique reserved staging, then publishes atomically without replacing a target.
fn copy_pack(source: &Path, target: &Path) -> io::Result<()> {
    reject_overlap(source, target)?;
    let staging = tempfile::Builder::new()
        .prefix(".shaderpack-")
        .suffix(STAGING_SUFFIX)
        .tempdir_in(target.parent().unwrap_or_else(|| Path::new(".")))?;
    let payload = staging.path().join("pack");
    if source.is_dir() {
        let folder = pack_folder(source.to_path_buf()).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "pack shaders folder is missing")
        })?;
        copy_folder(source, &payload, &folder.join("shaders"))?;
    } else {
        fs::copy(source, &payload)?;
    }
    assets::rename_no_replace(&payload, target)
}

/// Copies a folder tree, following only links along the selected shader root's path.
fn copy_folder(from: &Path, to: &Path, shaders: &Path) -> io::Result<()> {
    fs::create_dir(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let destination = to.join(entry.file_name());
        if kind.is_dir() || (shaders.starts_with(entry.path()) && entry.path().is_dir()) {
            copy_folder(&entry.path(), &destination, shaders)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
