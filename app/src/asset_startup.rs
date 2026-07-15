use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{AssetError, RuntimeAssets, RuntimeAtmosphereAssets};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::metrics::AssetMetrics;

pub const ASSET_PATH_ENVIRONMENT: &str = "RUST_MCBE_ASSETS";
pub const DEFAULT_ASSET_PATH: &str = ".local/assets/compiled/vanilla-v1001.mcbea";
pub const ATMOSPHERE_FILENAME: &str = "vanilla-v1.mcbeatm";
pub const ATMOSPHERE_COMPILE_COMMAND: &str = "make atmosphere-assets";
pub const FETCH_COMMAND: &str =
    "powershell -NoProfile -File scripts/fetch-vanilla-assets.ps1 -AcceptEula";
pub const COMPILE_COMMAND: &str = concat!(
    "cargo run -p assets --bin assetc -- compile ",
    "--pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack ",
    "--registry crates/assets/data/block-registry-v1001.bin ",
    "--light-registry crates/assets/data/block-light-registry-v1001.bin ",
    "--biome-registry crates/assets/data/biome-registry-v1001.bin ",
    "--out .local/assets/compiled/vanilla-v1001.mcbea"
);

const VANILLA_SOURCE_JSON: &str = include_str!("../../assets/vanilla-source.json");
const ATMOSPHERE_SHADER_SOURCE: &[u8] = include_bytes!("../../crates/render/src/atmosphere.wgsl");
const MAX_RUNTIME_BLOB_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ATMOSPHERE_BLOB_BYTES: u64 = 512 * 1024;

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
    pub atmosphere: LoadedAtmosphereAssets,
    pub metrics: AssetMetrics,
    pub selected_path: PathBuf,
    pub kind: LoadedAssetKind,
    pub notice: Option<String>,
}

pub struct LoadedAtmosphereAssets {
    runtime: Arc<RuntimeAtmosphereAssets>,
    identity: [u8; 32],
    selected_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtmosphereEvidence {
    pub envelope_sha256: String,
    pub shader_source_sha256: String,
}

impl LoadedAtmosphereAssets {
    #[must_use]
    pub fn selected_path(&self) -> &Path {
        &self.selected_path
    }

    pub fn into_parts(self) -> (Arc<RuntimeAtmosphereAssets>, [u8; 32]) {
        (self.runtime, self.identity)
    }

    #[must_use]
    pub fn evidence(&self) -> AtmosphereEvidence {
        AtmosphereEvidence {
            envelope_sha256: format_sha256(self.identity),
            shader_source_sha256: atmosphere_shader_source_sha256(),
        }
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        let evidence = self.evidence();
        format!(
            "loaded required atmosphere assets from {} (envelope_sha256={} shader_source_sha256={})",
            self.selected_path().display(),
            evidence.envelope_sha256,
            evidence.shader_source_sha256
        )
    }
}

#[must_use]
pub fn atmosphere_shader_source_sha256() -> String {
    format_sha256(Sha256::digest(ATMOSPHERE_SHADER_SOURCE).into())
}

fn format_sha256(identity: [u8; 32]) -> String {
    identity.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl std::fmt::Debug for LoadedAssets {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoadedAssets")
            .field("metrics", &self.metrics)
            .field("selected_path", &self.selected_path)
            .field("atmosphere_path", &self.atmosphere.selected_path)
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

    #[error(
        "could not decode compiled asset blob at {path}: {source}\nrebuild stale local assets with: {rebuild_command}"
    )]
    Decode {
        path: PathBuf,
        #[source]
        source: Box<AssetError>,
        rebuild_command: &'static str,
    },

    #[error("could not parse the checked-in vanilla source manifest: {0}")]
    SourceManifest(#[from] serde_json::Error),

    #[error(
        "could not read required atmosphere asset carrier at {path}: {source}\nrebuild local atmosphere assets with: {rebuild_command}"
    )]
    AtmosphereRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: &'static str,
    },

    #[error(
        "required atmosphere asset carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild local atmosphere assets with: {rebuild_command}"
    )]
    AtmosphereTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: &'static str,
    },

    #[error(
        "could not decode required atmosphere asset carrier at {path}: {source}\nrebuild local atmosphere assets with: {rebuild_command}"
    )]
    AtmosphereDecode {
        path: PathBuf,
        #[source]
        source: Box<AssetError>,
        rebuild_command: &'static str,
    },
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

#[must_use]
pub fn atmosphere_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(ATMOSPHERE_FILENAME)
}

pub fn load_runtime_assets(selection: AssetSelection) -> Result<LoadedAssets, AssetStartupError> {
    let source: VanillaSource = serde_json::from_str(VANILLA_SOURCE_JSON)?;
    let file = match File::open(&selection.path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let atmosphere = load_atmosphere_assets(&selection.path)?;
            return Ok(diagnostic_assets(selection, source, atmosphere));
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
                rebuild_command: COMPILE_COMMAND,
            })?,
        );
    let metrics = runtime_metrics(&runtime, source, blob_sha256);
    let atmosphere = load_atmosphere_assets(&selection.path)?;
    Ok(LoadedAssets {
        runtime,
        atmosphere,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::CompiledBlob,
        notice: None,
    })
}

fn load_atmosphere_assets(
    world_asset_path: &Path,
) -> Result<LoadedAtmosphereAssets, AssetStartupError> {
    let path = atmosphere_asset_path(world_asset_path);
    let file = File::open(&path).map_err(|source| AssetStartupError::AtmosphereRead {
        path: path.clone(),
        source,
        rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
    })?;
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::AtmosphereRead {
            path: path.clone(),
            source,
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        })?
        .len();
    if length > MAX_ATMOSPHERE_BLOB_BYTES {
        return Err(AssetStartupError::AtmosphereTooLarge {
            path,
            max_bytes: MAX_ATMOSPHERE_BLOB_BYTES,
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_ATMOSPHERE_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::AtmosphereRead {
            path: path.clone(),
            source,
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        })?;
    if bytes.len() as u64 > MAX_ATMOSPHERE_BLOB_BYTES {
        return Err(AssetStartupError::AtmosphereTooLarge {
            path,
            max_bytes: MAX_ATMOSPHERE_BLOB_BYTES,
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        });
    }
    let identity = Sha256::digest(&bytes).into();
    let runtime = Arc::new(RuntimeAtmosphereAssets::decode(&bytes).map_err(|source| {
        AssetStartupError::AtmosphereDecode {
            path: path.clone(),
            source: Box::new(source),
            rebuild_command: ATMOSPHERE_COMPILE_COMMAND,
        }
    })?);
    Ok(LoadedAtmosphereAssets {
        runtime,
        identity,
        selected_path: path,
    })
}

fn diagnostic_assets(
    selection: AssetSelection,
    source: VanillaSource,
    atmosphere: LoadedAtmosphereAssets,
) -> LoadedAssets {
    let runtime = Arc::new(RuntimeAssets::diagnostic());
    let metrics = runtime_metrics(&runtime, source, "diagnostic".to_owned());
    let notice = format!(
        "compiled vanilla assets were not found at {}; using the programmatic diagnostic texture\n\
         Fetch and compile the local vanilla pack explicitly (the app never downloads it):\n  {FETCH_COMMAND}\n  {COMPILE_COMMAND}",
        selection.path.display()
    );
    LoadedAssets {
        runtime,
        atmosphere,
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
    let pages = runtime.texture_pages();
    AssetMetrics {
        source_tag: source.tag,
        source_sha256: source.sha256,
        blob_sha256,
        texture_layers: pages.iter().map(|page| page.texture.layers).sum(),
        texture_pages: u32::try_from(pages.len()).unwrap_or(u32::MAX),
        texture_bytes_including_mips: pages
            .iter()
            .flat_map(|page| page.texture.mips.iter())
            .map(|mip| mip.rgba8.len() as u64)
            .sum(),
        material_count: u32::try_from(runtime.materials().len()).unwrap_or(u32::MAX),
        model_template_count: u32::try_from(runtime.model_templates().len()).unwrap_or(u32::MAX),
        model_quad_count: u32::try_from(runtime.model_quads().len()).unwrap_or(u32::MAX),
        animation_count: u32::try_from(runtime.animations().len()).unwrap_or(u32::MAX),
        animation_frame_count: u32::try_from(runtime.animation_frames().len()).unwrap_or(u32::MAX),
        missing_mapping_count: runtime.missing_count(),
        diagnostic_quad_count: 0,
    }
}
