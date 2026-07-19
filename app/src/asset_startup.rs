use std::{
    ffi::OsString,
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{
    AssetError, FontCatalogError, FontTexturePage, GlyphMetrics, HudCatalogError, RuntimeAssets,
    RuntimeAtmosphereAssets, RuntimeEntityAssets, RuntimeFontCatalog, RuntimeHudCatalog,
    encode_font_catalog,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::metrics::AssetMetrics;

pub const ASSET_PATH_ENVIRONMENT: &str = crate::acceptance::markers::ASSETS;
pub const DEFAULT_ASSET_PATH: &str = ".local/assets/compiled/vanilla-v1001.mcbea";
pub const ATMOSPHERE_FILENAME: &str = "vanilla-v1.mcbeatm";
pub const ATMOSPHERE_COMPILE_COMMAND: &str = "make atmosphere-assets";
pub const ENTITY_ASSETS_FILENAME: &str = "vanilla-v1.mcbeent";
pub const ENTITY_ASSETS_COMPILE_COMMAND: &str = "make entity-assets";
pub const FONT_ASSETS_FILENAME: &str = "ui-inter-v1.mcbefont";
pub const FONT_ASSETS_COMPILE_COMMAND: &str = "make font-assets";
pub const LOCAL_FONT_ASSETS_FILENAME: &str = "vanilla-v1.mcbefont";
pub const LOCAL_FONT_ASSETS_COMPILE_COMMAND: &str =
    "make font-assets-local FONT_PACK_DIR=<reviewed-font-pack>";
pub const HUD_ASSETS_FILENAME: &str = "vanilla-v1.mcbehud";
pub const HUD_ASSETS_COMPILE_COMMAND: &str = "make hud-assets";
pub const FETCH_COMMAND: &str =
    "powershell -NoProfile -File scripts/fetch-vanilla-assets.ps1 -AcceptEula";
pub const COMPILE_COMMAND: &str = concat!(
    "cargo run -p asset-compiler --bin assetc -- compile ",
    "--pack .local/assets/bedrock-samples/v1.26.30.32-preview/full/resource_pack ",
    "--registry crates/assets/data/block-registry-v1001.bin ",
    "--light-registry crates/assets/data/block-light-registry-v1001.bin ",
    "--biome-registry crates/assets/data/biome-registry-v1001.bin ",
    "--out .local/assets/compiled/vanilla-v1001.mcbea"
);

const VANILLA_SOURCE_JSON: &str = include_str!("../../assets/vanilla-source.json");
const UI_FONT_SOURCE_JSON: &str = include_str!("../../assets/ui-font-source.json");
const ATMOSPHERE_SHADER_SOURCE: &[u8] = include_bytes!("../../crates/render/src/atmosphere.wgsl");
const CLOUD_SHADER_SOURCE: &[u8] = include_bytes!("../../crates/render/src/cloud.wgsl");
const MAX_RUNTIME_BLOB_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ATMOSPHERE_BLOB_BYTES: u64 = 512 * 1024;
const MAX_ENTITY_ASSET_BLOB_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FONT_ASSET_BLOB_BYTES: u64 = 128 * 1024 * 1024;
const MAX_HUD_ASSET_BLOB_BYTES: u64 = 8 * 1024 * 1024;

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
    pub entities: LoadedEntityAssets,
    pub fonts: LoadedFontAssets,
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

pub struct LoadedEntityAssets {
    runtime: Arc<RuntimeEntityAssets>,
    identity: [u8; 32],
    selected_path: PathBuf,
}

pub struct LoadedFontAssets {
    runtime: Arc<RuntimeFontCatalog>,
    selected_path: PathBuf,
    diagnostic: bool,
}

pub struct LoadedHudAssets {
    runtime: Arc<RuntimeHudCatalog>,
    selected_path: PathBuf,
}

impl LoadedHudAssets {
    #[must_use]
    pub fn runtime(&self) -> &Arc<RuntimeHudCatalog> {
        &self.runtime
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        format!(
            "loaded local vanilla HUD assets from {} (source_manifest_sha256={})",
            self.selected_path.display(),
            format_sha256(self.runtime.source_manifest_sha256())
        )
    }

    pub fn into_runtime(self) -> Arc<RuntimeHudCatalog> {
        self.runtime
    }
}

impl LoadedFontAssets {
    #[must_use]
    pub fn selected_path(&self) -> &Path {
        &self.selected_path
    }

    #[must_use]
    pub const fn is_diagnostic(&self) -> bool {
        self.diagnostic
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        if self.diagnostic {
            return format!(
                "font asset carrier was not found at {}; using bounded diagnostic font fallback; build the reviewed Inter carrier with: {}",
                self.selected_path.display(),
                FONT_ASSETS_COMPILE_COMMAND
            );
        }
        format!(
            "loaded required font assets from {}",
            self.selected_path.display()
        )
    }

    pub fn into_runtime(self) -> Arc<RuntimeFontCatalog> {
        self.runtime
    }
}

impl LoadedEntityAssets {
    #[must_use]
    pub fn selected_path(&self) -> &Path {
        &self.selected_path
    }

    #[must_use]
    pub fn runtime(&self) -> &Arc<RuntimeEntityAssets> {
        &self.runtime
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        format!(
            "ENTITY_ASSET_EVIDENCE envelope_sha256={} source_manifest_sha256={} sources={} symbols={}",
            format_sha256(self.identity),
            format_sha256(self.runtime.source_manifest_sha256()),
            self.runtime.sources().len(),
            self.runtime.symbols().len()
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AtmosphereEvidence {
    pub envelope_sha256: String,
    pub shader_source_sha256: String,
    pub cloud_shader_source_sha256: String,
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
            cloud_shader_source_sha256: cloud_shader_source_sha256(),
        }
    }

    #[must_use]
    pub fn startup_summary(&self) -> String {
        let evidence = self.evidence();
        format!(
            "ATMOSPHERE_EVIDENCE envelope_sha256={} shader_source_sha256={} cloud_shader_source_sha256={}",
            evidence.envelope_sha256,
            evidence.shader_source_sha256,
            evidence.cloud_shader_source_sha256
        )
    }
}

#[must_use]
pub fn atmosphere_shader_source_sha256() -> String {
    format_sha256(Sha256::digest(ATMOSPHERE_SHADER_SOURCE).into())
}

#[must_use]
pub fn cloud_shader_source_sha256() -> String {
    format_sha256(Sha256::digest(CLOUD_SHADER_SOURCE).into())
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
            .field("entity_asset_path", &self.entities.selected_path)
            .field("font_asset_path", &self.fonts.selected_path)
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

    #[error(
        "could not read required entity asset carrier at {path}: {source}\nrebuild local entity assets with: {rebuild_command}"
    )]
    EntityAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: &'static str,
    },

    #[error(
        "required entity asset carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild local entity assets with: {rebuild_command}"
    )]
    EntityAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: &'static str,
    },

    #[error(
        "could not decode required entity asset carrier at {path}: {source}\nrebuild local entity assets with: {rebuild_command}"
    )]
    EntityAssetsDecode {
        path: PathBuf,
        #[source]
        source: Box<AssetError>,
        rebuild_command: &'static str,
    },

    #[error(
        "required entity asset carrier at {path} has stale provenance (expected source manifest SHA-256 {expected}, found {actual})\nrebuild local entity assets with: {rebuild_command}"
    )]
    EntityAssetsProvenance {
        path: PathBuf,
        expected: String,
        actual: String,
        rebuild_command: &'static str,
    },

    #[error(
        "could not read required font asset carrier at {path}: {source}\nrebuild local font assets with: {rebuild_command}"
    )]
    FontAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: &'static str,
    },

    #[error(
        "required font asset carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild local font assets with: {rebuild_command}"
    )]
    FontAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: &'static str,
    },

    #[error(
        "could not decode required font asset carrier at {path}: {source}\nrebuild local font assets with: {rebuild_command}"
    )]
    FontAssetsDecode {
        path: PathBuf,
        #[source]
        source: Box<FontCatalogError>,
        rebuild_command: &'static str,
    },

    #[error(
        "could not read local HUD asset carrier at {path}: {source}\nrebuild local HUD assets with: {rebuild_command}"
    )]
    HudAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: &'static str,
    },

    #[error(
        "local HUD asset carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild local HUD assets with: {rebuild_command}"
    )]
    HudAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: &'static str,
    },

    #[error(
        "could not decode local HUD asset carrier at {path}: {source}\nrebuild local HUD assets with: {rebuild_command}"
    )]
    HudAssetsDecode {
        path: PathBuf,
        #[source]
        source: HudCatalogError,
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
pub fn select_asset_path_from_environment(command_line: Option<&Path>) -> AssetSelection {
    let current_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let executable = std::env::current_exe().unwrap_or_default();
    select_asset_path_in_context(
        command_line,
        std::env::var_os(ASSET_PATH_ENVIRONMENT),
        &current_directory,
        &executable,
    )
}

#[must_use]
pub fn atmosphere_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(ATMOSPHERE_FILENAME)
}

#[must_use]
pub fn entity_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(ENTITY_ASSETS_FILENAME)
}

#[must_use]
pub fn font_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(FONT_ASSETS_FILENAME)
}

#[must_use]
pub fn local_font_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(LOCAL_FONT_ASSETS_FILENAME)
}

#[must_use]
pub fn hud_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(HUD_ASSETS_FILENAME)
}

pub fn load_hud_assets(
    world_asset_path: &Path,
) -> Result<Option<LoadedHudAssets>, AssetStartupError> {
    let path = hud_asset_path(world_asset_path);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(AssetStartupError::HudAssetsRead {
                path,
                source,
                rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
            });
        }
    };
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::HudAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
        })?
        .len();
    if length > MAX_HUD_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::HudAssetsTooLarge {
            path,
            max_bytes: MAX_HUD_ASSET_BLOB_BYTES,
            rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_HUD_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::HudAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
        })?;
    if bytes.len() as u64 > MAX_HUD_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::HudAssetsTooLarge {
            path,
            max_bytes: MAX_HUD_ASSET_BLOB_BYTES,
            rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
        });
    }
    let runtime =
        RuntimeHudCatalog::decode(&bytes).map_err(|source| AssetStartupError::HudAssetsDecode {
            path: path.clone(),
            source,
            rebuild_command: HUD_ASSETS_COMPILE_COMMAND,
        })?;
    Ok(Some(LoadedHudAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
    }))
}

pub fn load_runtime_assets(selection: AssetSelection) -> Result<LoadedAssets, AssetStartupError> {
    let source: VanillaSource = serde_json::from_str(VANILLA_SOURCE_JSON)?;
    let file = match File::open(&selection.path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let atmosphere = load_atmosphere_assets(&selection.path)?;
            let entities = load_entity_assets(&selection.path)?;
            let fonts = load_font_assets(&selection.path)?;
            return Ok(diagnostic_assets(
                selection, source, atmosphere, entities, fonts,
            ));
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
    let entities = load_entity_assets(&selection.path)?;
    let fonts = load_font_assets(&selection.path)?;
    Ok(LoadedAssets {
        runtime,
        atmosphere,
        entities,
        fonts,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::CompiledBlob,
        notice: None,
    })
}

fn load_entity_assets(world_asset_path: &Path) -> Result<LoadedEntityAssets, AssetStartupError> {
    let path = entity_asset_path(world_asset_path);
    let file = File::open(&path).map_err(|source| AssetStartupError::EntityAssetsRead {
        path: path.clone(),
        source,
        rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
    })?;
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::EntityAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        })?
        .len();
    if length > MAX_ENTITY_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::EntityAssetsTooLarge {
            path,
            max_bytes: MAX_ENTITY_ASSET_BLOB_BYTES,
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_ENTITY_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::EntityAssetsRead {
            path: path.clone(),
            source,
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        })?;
    if bytes.len() as u64 > MAX_ENTITY_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::EntityAssetsTooLarge {
            path,
            max_bytes: MAX_ENTITY_ASSET_BLOB_BYTES,
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        });
    }
    let identity = Sha256::digest(&bytes).into();
    let runtime = Arc::new(RuntimeEntityAssets::decode(&bytes).map_err(|source| {
        AssetStartupError::EntityAssetsDecode {
            path: path.clone(),
            source: Box::new(source),
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        }
    })?);
    let expected_manifest_sha256 = canonical_source_manifest_sha256(VANILLA_SOURCE_JSON);
    let actual_manifest_sha256 = runtime.source_manifest_sha256();
    if actual_manifest_sha256 != expected_manifest_sha256 {
        return Err(AssetStartupError::EntityAssetsProvenance {
            path,
            expected: format_sha256(expected_manifest_sha256),
            actual: format_sha256(actual_manifest_sha256),
            rebuild_command: ENTITY_ASSETS_COMPILE_COMMAND,
        });
    }
    Ok(LoadedEntityAssets {
        runtime,
        identity,
        selected_path: path,
    })
}

fn load_font_assets(world_asset_path: &Path) -> Result<LoadedFontAssets, AssetStartupError> {
    let local_path = local_font_asset_path(world_asset_path);
    let (path, file, source_manifest, rebuild_command) = match File::open(&local_path) {
        Ok(file) => (
            local_path,
            file,
            VANILLA_SOURCE_JSON,
            LOCAL_FONT_ASSETS_COMPILE_COMMAND,
        ),
        Err(source) if source.kind() == io::ErrorKind::NotFound => {
            let path = font_asset_path(world_asset_path);
            let file = match File::open(&path) {
                Ok(file) => file,
                Err(source) if source.kind() == io::ErrorKind::NotFound => {
                    return diagnostic_font_assets(path);
                }
                Err(source) => {
                    return Err(AssetStartupError::FontAssetsRead {
                        path,
                        source,
                        rebuild_command: FONT_ASSETS_COMPILE_COMMAND,
                    });
                }
            };
            (path, file, UI_FONT_SOURCE_JSON, FONT_ASSETS_COMPILE_COMMAND)
        }
        Err(source) => {
            return Err(AssetStartupError::FontAssetsRead {
                path: local_path,
                source,
                rebuild_command: LOCAL_FONT_ASSETS_COMPILE_COMMAND,
            });
        }
    };
    let length = file
        .metadata()
        .map_err(|source| AssetStartupError::FontAssetsRead {
            path: path.clone(),
            source,
            rebuild_command,
        })?
        .len();
    if length > MAX_FONT_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::FontAssetsTooLarge {
            path,
            max_bytes: MAX_FONT_ASSET_BLOB_BYTES,
            rebuild_command,
        });
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_FONT_ASSET_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::FontAssetsRead {
            path: path.clone(),
            source,
            rebuild_command,
        })?;
    if bytes.len() as u64 > MAX_FONT_ASSET_BLOB_BYTES {
        return Err(AssetStartupError::FontAssetsTooLarge {
            path,
            max_bytes: MAX_FONT_ASSET_BLOB_BYTES,
            rebuild_command,
        });
    }
    let expected_manifest_sha256 = canonical_source_manifest_sha256(source_manifest);
    let runtime =
        RuntimeFontCatalog::decode(&bytes, expected_manifest_sha256).map_err(|source| {
            AssetStartupError::FontAssetsDecode {
                path: path.clone(),
                source: Box::new(source),
                rebuild_command,
            }
        })?;
    Ok(LoadedFontAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
        diagnostic: false,
    })
}

fn diagnostic_font_assets(path: PathBuf) -> Result<LoadedFontAssets, AssetStartupError> {
    const DIAGNOSTIC_MANIFEST: [u8; 32] = [0xd1; 32];
    let rgba8 = vec![255, 255, 255, 255].into_boxed_slice();
    let page = FontTexturePage {
        source_path: "font/builtin-diagnostic.png".into(),
        source_bytes: 4,
        source_sha256: Sha256::digest(&rgba8).into(),
        pixels_sha256: Sha256::digest(&rgba8).into(),
        width: 1,
        height: 1,
        rgba8,
    };
    let glyph = GlyphMetrics {
        codepoint: '\u{fffd}',
        page: 0,
        uv: [0, 0, 1, 1],
        bearing: [0, 0],
        advance_64: 64,
    };
    let bytes = encode_font_catalog(DIAGNOSTIC_MANIFEST, &[glyph], &[page]).map_err(|source| {
        AssetStartupError::FontAssetsDecode {
            path: path.clone(),
            source: Box::new(source),
            rebuild_command: FONT_ASSETS_COMPILE_COMMAND,
        }
    })?;
    let runtime = RuntimeFontCatalog::decode(&bytes, DIAGNOSTIC_MANIFEST).map_err(|source| {
        AssetStartupError::FontAssetsDecode {
            path: path.clone(),
            source: Box::new(source),
            rebuild_command: FONT_ASSETS_COMPILE_COMMAND,
        }
    })?;
    Ok(LoadedFontAssets {
        runtime: Arc::new(runtime),
        selected_path: path,
        diagnostic: true,
    })
}

fn canonical_source_manifest_sha256(source: &str) -> [u8; 32] {
    let source = source.as_bytes();
    if !source.contains(&b'\r') {
        return Sha256::digest(source).into();
    }
    let mut canonical = Vec::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        match source[index] {
            b'\r' if source.get(index + 1) == Some(&b'\n') => {
                canonical.push(b'\n');
                index += 2;
            }
            b'\r' | b'\n' => return Sha256::digest(source).into(),
            byte => {
                canonical.push(byte);
                index += 1;
            }
        }
    }
    Sha256::digest(canonical).into()
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
    entities: LoadedEntityAssets,
    fonts: LoadedFontAssets,
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
        entities,
        fonts,
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
        diagnostic_attribution: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_source_manifest_sha256;

    #[test]
    fn canonical_source_manifest_hash_is_line_ending_invariant() {
        assert_eq!(
            canonical_source_manifest_sha256("{\r\n  \"schema\": 1\r\n}\r\n"),
            canonical_source_manifest_sha256("{\n  \"schema\": 1\n}\n")
        );
    }

    #[test]
    fn mixed_source_manifest_line_endings_do_not_match_the_canonical_pin() {
        assert_ne!(
            canonical_source_manifest_sha256("{\r\n  \"schema\": 1\n}\r\n"),
            canonical_source_manifest_sha256("{\n  \"schema\": 1\n}\n")
        );
    }
}
