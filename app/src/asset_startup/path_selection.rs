//! Asset-path selection, split from the asset-startup root to honor the
//! production line budget. Pure path resolution: no filesystem access here.

use std::{ffi::OsString, path::Path, path::PathBuf};

pub const DEFAULT_ASSET_PATH: &str = ".local/assets/compiled/vanilla-v2168.mcbea";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetPathSource {
    CommandLine,
    Environment,
    Default,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetSelection {
    pub path: PathBuf,
    pub source: AssetPathSource,
}

#[must_use]
pub fn select_asset_path(
    command_line: Option<&Path>,
    environment: Option<OsString>,
) -> AssetSelection {
    select_asset_path_with_default(command_line, environment, Path::new(DEFAULT_ASSET_PATH))
}

#[must_use]
pub fn select_asset_path_with_default(
    command_line: Option<&Path>,
    environment: Option<OsString>,
    default_path: &Path,
) -> AssetSelection {
    if let Some(path) = command_line {
        return AssetSelection {
            path: path.to_owned(),
            source: AssetPathSource::CommandLine,
        };
    }
    if let Some(path) = environment.filter(|path| !path.is_empty()) {
        return AssetSelection {
            path: PathBuf::from(path),
            source: AssetPathSource::Environment,
        };
    }
    AssetSelection {
        path: default_path.to_owned(),
        source: AssetPathSource::Default,
    }
}

#[must_use]
pub fn select_asset_path_in_context(
    command_line: Option<&Path>,
    environment: Option<OsString>,
    current_directory: &Path,
    executable: &Path,
) -> AssetSelection {
    let mut selection = select_asset_path(command_line, environment);
    if selection.source != AssetPathSource::Default || selection.path.is_absolute() {
        return selection;
    }
    if current_directory.join(&selection.path).is_file() {
        return selection;
    }
    let Some(project_root) = executable
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
    else {
        return selection;
    };
    let executable_relative = project_root.join(&selection.path);
    if executable_relative.is_file() {
        selection.path = executable_relative;
    }
    selection
}

#[must_use]
pub fn select_asset_path_from_environment(
    command_line: Option<&Path>,
    default_path: &Path,
) -> AssetSelection {
    select_asset_path_with_default(
        command_line,
        std::env::var_os(super::ASSET_PATH_ENVIRONMENT),
        default_path,
    )
}
