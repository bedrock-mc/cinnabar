//! Opt-in private fixtures of sources the session already admitted.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use resource_pack::LayeredPackView;

use super::ServerUiPack;

const DIRECTORY_ENV: &str = "CINNABAR_ADMITTED_PACK_DUMP_DIR";
const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CAPTURE_BYTES: u64 = 64 * 1024 * 1024;
const JSON_PREFIXES: [&str; 6] = [
    "ui/",
    "entity/",
    "animations/",
    "animation_controllers/",
    "render_controllers/",
    "models/",
];
static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(0);

pub(super) fn write(view: &LayeredPackView, ui: Option<&ServerUiPack>) {
    let Some(directory) = std::env::var_os(DIRECTORY_ENV) else {
        return;
    };
    match write_at(Path::new(&directory), view, ui) {
        Ok((directory, files)) => bevy::log::info!(
            directory = %directory.display(),
            files,
            "admitted server pack fixture captured"
        ),
        Err(error) => bevy::log::warn!(
            setting = DIRECTORY_ENV,
            %error,
            "admitted server pack fixture was not captured"
        ),
    }
}

fn write_at(
    directory: &Path,
    view: &LayeredPackView,
    ui: Option<&ServerUiPack>,
) -> io::Result<(PathBuf, usize)> {
    let directory = existing_private_directory(directory)?;
    let capture = loop {
        let index = NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed);
        let path = directory.join(format!("stack-{index:04}"));
        match create_private_directory(&path) {
            Ok(()) => break path,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    };
    let mut files = 0;
    let mut total = 0u64;
    for (index, source) in view.layers().enumerate() {
        let layer = capture.join(format!("layer-{index:04}"));
        create_private_directory(&layer)?;
        let ui_files = ui.and_then(|pack| pack.ui_layers.get(index));
        let paths: BTreeSet<_> = source
            .files_under("")
            .iter()
            .copied()
            .filter(|path| fixture_source(path))
            .map(str::to_owned)
            .chain(ui_files.into_iter().flatten().map(|(path, _)| path.clone()))
            .collect();
        for path in paths {
            let Some(relative) = relative_path(&path) else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unsafe pack document path",
                ));
            };
            let remaining = MAX_CAPTURE_BYTES
                .saturating_sub(total)
                .min(MAX_DOCUMENT_BYTES);
            let existing = ui_files
                .into_iter()
                .flatten()
                .find_map(|(name, bytes)| (name == &path).then_some(bytes.as_slice()));
            let loaded;
            let bytes = match existing {
                Some(bytes) if bytes.len() as u64 <= remaining => bytes,
                Some(_) => {
                    return Err(io::Error::other("pack fixture exceeds its byte limit"));
                }
                None => {
                    loaded = source
                        .read_file_with_limit(&path, remaining)
                        .map_err(io::Error::other)?;
                    let Some(bytes) = loaded.as_deref() else {
                        continue;
                    };
                    bytes
                }
            };
            let output = layer.join(relative);
            let parent = output.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "pack document lacks a parent")
            })?;
            create_parents(&layer, parent)?;
            let mut file = private_file(&output)?;
            file.write_all(bytes)?;
            total += bytes.len() as u64;
            files += 1;
        }
    }
    Ok((capture, files))
}

fn fixture_source(path: &str) -> bool {
    let font = path.starts_with("font/")
        || path
            .strip_prefix("texts/")
            .and_then(|localized| localized.split_once('/'))
            .is_some_and(|(_, member)| member.starts_with("font/"));
    (path.ends_with(".json") && JSON_PREFIXES.iter().any(|prefix| path.starts_with(prefix)))
        || (path.starts_with("materials/") && path.ends_with(".material"))
        || (font
            && [".png", ".tga", ".json"]
                .iter()
                .any(|suffix| path.ends_with(suffix)))
        || path.rsplit('/').next() == Some("font_metadata.json")
}

fn relative_path(path: &str) -> Option<&Path> {
    if path.is_empty()
        || path.contains(['\\', ':', '\0'])
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    let path = Path::new(path);
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
        .then_some(path)
}

fn existing_private_directory(path: &Path) -> io::Result<PathBuf> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "fixture destination is not a directory",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fixture destination must be private",
            ));
        }
    }
    fs::canonicalize(path)
}

fn create_private_directory(path: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn create_parents(root: &Path, parent: &Path) -> io::Result<()> {
    let mut path = root.to_owned();
    for component in parent
        .strip_prefix(root)
        .map_err(io::Error::other)?
        .components()
    {
        path.push(component);
        match create_private_directory(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let metadata = fs::symlink_metadata(&path)?;
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "unsafe fixture directory",
                    ));
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn private_file(path: &Path) -> io::Result<File> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::{fixture_source, relative_path};

    #[test]
    fn fixtures_include_font_and_material_sources_without_other_payloads() {
        for path in [
            "ui/server_form.json",
            "entity/player.json",
            "models/entity/avatar.json",
            "materials/actors.material",
            "font/glyph_E2.png",
            "font/default8.tga",
            "font/font_metadata.json",
            "texts/ja_JP/font/glyph_32.png",
            "fonts/custom/font_metadata.json",
        ] {
            assert!(fixture_source(path), "{path}");
        }
        for path in [
            "textures/entity/avatar.png",
            "shaders/actor.json",
            "materials/actors.json",
            "font/custom.ttf",
            "font/contents.json.key",
            "contents.json",
            "manifest.json",
            "texts/en_US.lang",
        ] {
            assert!(!fixture_source(path), "{path}");
        }
    }

    #[test]
    fn fixture_paths_keep_only_relative_archive_members() {
        for path in [
            "ui/server_form.json",
            "ui/custom/layout.json",
            "forms/layout.json",
        ] {
            assert!(relative_path(path).is_some(), "{path}");
        }
        for path in [
            "",
            "/ui/form.json",
            "../form.json",
            "ui/../form.json",
            "ui/./form.json",
            "ui//form.json",
            "C:/form.json",
            "ui\\form.json",
            "ui/\0.json",
        ] {
            assert!(relative_path(path).is_none(), "{path:?}");
        }
    }
}
