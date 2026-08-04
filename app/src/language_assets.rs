use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{LanguageCatalogError, RuntimeLanguageCatalog};
use thiserror::Error;

pub const LANGUAGE_ASSETS_FILENAME: &str = "vanilla-v1.mcbclang";
pub const LANGUAGE_ASSETS_COMPILE_COMMAND: &str = "make language-assets";
const MAX_LANGUAGE_ASSET_BLOB_BYTES: u64 = 16 * 1024 * 1024;

pub struct LoadedLanguageAssets {
    runtime: Arc<RuntimeLanguageCatalog>,
    selected_path: PathBuf,
}

impl LoadedLanguageAssets {
    #[must_use]
    pub fn selected_path(&self) -> &Path {
        &self.selected_path
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        format!(
            "loaded vanilla translation assets from {} ({} entries)",
            self.selected_path.display(),
            self.runtime.translations().len()
        )
    }

    pub fn into_runtime(self) -> Arc<RuntimeLanguageCatalog> {
        self.runtime
    }
}

#[derive(Debug, Error)]
pub enum LanguageAssetsError {
    #[error(
        "could not read required language asset carrier at {path}: {source}\nrebuild local language assets with: {rebuild_command}"
    )]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: &'static str,
    },
    #[error(
        "required language asset carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild local language assets with: {rebuild_command}"
    )]
    TooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: &'static str,
    },
    #[error(
        "could not decode required language asset carrier at {path}: {source}\nrebuild local language assets with: {rebuild_command}"
    )]
    Decode {
        path: PathBuf,
        #[source]
        source: Box<LanguageCatalogError>,
        rebuild_command: &'static str,
    },
}

#[must_use]
pub fn language_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(LANGUAGE_ASSETS_FILENAME)
}

pub fn load_language_assets(
    world_asset_path: &Path,
    expected_manifest_sha256: [u8; 32],
) -> Result<LoadedLanguageAssets, LanguageAssetsError> {
    let path = language_asset_path(world_asset_path);
    let file = File::open(&path).map_err(|source| LanguageAssetsError::Read {
        path: path.clone(),
        source,
        rebuild_command: LANGUAGE_ASSETS_COMPILE_COMMAND,
    })?;
    let length = file
        .metadata()
        .map_err(|source| LanguageAssetsError::Read {
            path: path.clone(),
            source,
            rebuild_command: LANGUAGE_ASSETS_COMPILE_COMMAND,
        })?
        .len();
    if length > MAX_LANGUAGE_ASSET_BLOB_BYTES {
        return Err(LanguageAssetsError::TooLarge {
            path,
            max_bytes: MAX_LANGUAGE_ASSET_BLOB_BYTES,
            rebuild_command: LANGUAGE_ASSETS_COMPILE_COMMAND,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_LANGUAGE_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| LanguageAssetsError::Read {
            path: path.clone(),
            source,
            rebuild_command: LANGUAGE_ASSETS_COMPILE_COMMAND,
        })?;
    if bytes.len() as u64 > MAX_LANGUAGE_ASSET_BLOB_BYTES {
        return Err(LanguageAssetsError::TooLarge {
            path,
            max_bytes: MAX_LANGUAGE_ASSET_BLOB_BYTES,
            rebuild_command: LANGUAGE_ASSETS_COMPILE_COMMAND,
        });
    }
    let runtime =
        RuntimeLanguageCatalog::decode(&bytes, expected_manifest_sha256).map_err(|source| {
            LanguageAssetsError::Decode {
                path: path.clone(),
                source: Box::new(source),
                rebuild_command: LANGUAGE_ASSETS_COMPILE_COMMAND,
            }
        })?;
    Ok(LoadedLanguageAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
    })
}
