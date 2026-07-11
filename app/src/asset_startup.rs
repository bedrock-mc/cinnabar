use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{AssetError, RuntimeAssets};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::metrics::AssetMetrics;

pub const ASSET_PATH_ENVIRONMENT: &str = "RUST_MCBE_ASSETS";
pub const DEFAULT_ASSET_PATH: &str = ".local/assets/compiled/vanilla-v1001.mcbea";
pub const FETCH_COMMAND: &str =
    "powershell -NoProfile -File scripts/fetch-vanilla-assets.ps1 -AcceptEula";
pub const COMPILE_COMMAND: &str = concat!(
    "cargo run -p assets --bin assetc -- compile ",
    "--pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack ",
    "--registry crates/assets/data/block-registry-v1001.bin ",
    "--out .local/assets/compiled/vanilla-v1001.mcbea"
);

const VANILLA_SOURCE_JSON: &str = include_str!("../../assets/vanilla-source.json");
const MAX_RUNTIME_BLOB_BYTES: u64 = 16 * 1024 * 1024;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadedAssetKind {
    CompiledBlob,
    Diagnostic,
}

pub struct LoadedAssets {
    pub runtime: Arc<RuntimeAssets>,
    pub metrics: AssetMetrics,
    pub selected_path: PathBuf,
    pub kind: LoadedAssetKind,
    pub notice: Option<String>,
}

impl std::fmt::Debug for LoadedAssets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedAssets")
            .field("metrics", &self.metrics)
            .field("selected_path", &self.selected_path)
            .field("kind", &self.kind)
            .field("notice", &self.notice)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
pub enum AssetStartupError {
    #[error("could not read compiled asset blob at {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("compiled asset blob at {path} exceeds the {max_bytes}-byte startup limit")]
    TooLarge { path: PathBuf, max_bytes: u64 },

    #[error("could not decode compiled asset blob at {path}: {source}")]
    Decode {
        path: PathBuf,
        #[source]
        source: Box<AssetError>,
    },

    #[error("could not parse the checked-in vanilla source manifest: {0}")]
    SourceManifest(#[from] serde_json::Error),
}

#[derive(Deserialize)]
struct VanillaSource {
    tag: String,
    sha256: String,
}

#[must_use]
pub fn select_asset_path(
    command_line: Option<&Path>,
    environment: Option<OsString>,
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
        path: PathBuf::from(DEFAULT_ASSET_PATH),
        source: AssetPathSource::Default,
    }
}

#[must_use]
pub fn select_asset_path_from_environment(command_line: Option<&Path>) -> AssetSelection {
    select_asset_path(command_line, std::env::var_os(ASSET_PATH_ENVIRONMENT))
}

pub fn load_runtime_assets(selection: AssetSelection) -> Result<LoadedAssets, AssetStartupError> {
    let source: VanillaSource = serde_json::from_str(VANILLA_SOURCE_JSON)?;
    let file = match File::open(&selection.path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(diagnostic_assets(selection, source));
        }
        Err(source) => {
            return Err(AssetStartupError::Read {
                path: selection.path,
                source,
            });
        }
    };
    if file
        .metadata()
        .map_err(|source| AssetStartupError::Read {
            path: selection.path.clone(),
            source,
        })?
        .len()
        > MAX_RUNTIME_BLOB_BYTES
    {
        return Err(AssetStartupError::TooLarge {
            path: selection.path,
            max_bytes: MAX_RUNTIME_BLOB_BYTES,
        });
    }

    let mut bytes = Vec::new();
    file.take(MAX_RUNTIME_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::Read {
            path: selection.path.clone(),
            source,
        })?;
    if bytes.len() as u64 > MAX_RUNTIME_BLOB_BYTES {
        return Err(AssetStartupError::TooLarge {
            path: selection.path,
            max_bytes: MAX_RUNTIME_BLOB_BYTES,
        });
    }

    let blob_sha256 = format!("{:x}", Sha256::digest(&bytes));
    let runtime =
        Arc::new(
            RuntimeAssets::decode(&bytes).map_err(|source| AssetStartupError::Decode {
                path: selection.path.clone(),
                source: Box::new(source),
            })?,
        );
    let metrics = runtime_metrics(&runtime, source, blob_sha256);
    Ok(LoadedAssets {
        runtime,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::CompiledBlob,
        notice: None,
    })
}

fn diagnostic_assets(selection: AssetSelection, source: VanillaSource) -> LoadedAssets {
    let runtime = Arc::new(RuntimeAssets::diagnostic());
    let metrics = runtime_metrics(&runtime, source, "diagnostic".to_owned());
    let notice = format!(
        "compiled vanilla assets were not found at {}; using the programmatic diagnostic texture\n\
         Fetch and compile the local vanilla pack explicitly (the app never downloads it):\n  {FETCH_COMMAND}\n  {COMPILE_COMMAND}",
        selection.path.display()
    );
    LoadedAssets {
        runtime,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::Diagnostic,
        notice: Some(notice),
    }
}

fn runtime_metrics(
    runtime: &RuntimeAssets,
    source: VanillaSource,
    blob_sha256: String,
) -> AssetMetrics {
    let textures = runtime.texture_array();
    AssetMetrics {
        source_tag: source.tag,
        source_sha256: source.sha256,
        blob_sha256,
        texture_layers: textures.layers,
        texture_bytes_including_mips: textures.mips.iter().map(|mip| mip.rgba8.len() as u64).sum(),
        material_count: u32::try_from(runtime.materials().len()).unwrap_or(u32::MAX),
        missing_mapping_count: runtime.missing_count(),
        diagnostic_quad_count: 0,
    }
}
