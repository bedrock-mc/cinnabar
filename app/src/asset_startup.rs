use std::{
    fs::File,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{
    AssetError, FontCatalogError, HudCatalogError, RuntimeAssets, RuntimeAtmosphereAssets,
    RuntimeEntityAssets, RuntimeFontCatalog,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use diagnostics::metrics::AssetMetrics;

mod font_fallback;
use font_fallback::diagnostic_font_assets;
mod optional_carriers;
pub(crate) use optional_carriers::shell_quote_path;
use optional_carriers::{
    load_atmosphere_assets, load_entity_assets, load_font_assets, load_material_keys,
    load_vanilla_entity_refs,
};
mod world_provenance;
pub use world_provenance::pinned_world_provenance;
pub(crate) use world_provenance::{active_content_registry_protocol, pinned_block_registry_bytes};

pub const ATMOSPHERE_FILENAME: &str = assets::carriers::ATMOSPHERE.output;
pub const ATMOSPHERE_COMPILE_COMMAND: &str = "make atmosphere-assets";
pub const ENTITY_ASSETS_FILENAME: &str = assets::carriers::ENTITY.output;
pub const ENTITY_ASSETS_COMPILE_COMMAND: &str = "make entity-assets";
pub const FONT_ASSETS_FILENAME: &str = assets::carriers::FONT.output;
pub const FONT_ASSETS_COMPILE_COMMAND: &str = "make font-assets";
pub const LOCAL_FONT_ASSETS_FILENAME: &str = "vanilla-v1.mcbefont";
pub const LOCAL_FONT_ASSETS_COMPILE_COMMAND: &str =
    "make font-assets-local FONT_PACK_DIR=<reviewed-font-pack>";
pub const HUD_ASSETS_FILENAME: &str = assets::carriers::HUD.output;
pub const HUD_ASSETS_REPORT_FILENAME: &str = assets::carriers::HUD.report.unwrap();
pub const HUD_ASSETS_COMPILE_COMMAND: &str = "make hud-assets";
pub const AUDIO_ASSETS_FILENAME: &str = assets::carriers::AUDIO.output;
pub const AUDIO_ASSETS_COMPILE_COMMAND: &str = "make audio-assets";
pub const FETCH_COMMAND: &str = "make vanilla-assets";
pub const COMPILE_COMMAND: &str = "make world-assets";

const VANILLA_SOURCE_JSON: &str = assets::VANILLA_SOURCE_MANIFEST;
const FONT_SOURCE_JSON: &str = include_str!("../../assets/cinnangles-sans-source.json");
const ATMOSPHERE_SHADER_SOURCE: &[u8] = include_bytes!("../../crates/render/src/atmosphere.wgsl");
const CLOUD_SHADER_SOURCE: &[u8] = include_bytes!("../../crates/render/src/cloud.wgsl");
const MAX_RUNTIME_BLOB_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ATMOSPHERE_BLOB_BYTES: u64 = 512 * 1024;
const MAX_ENTITY_ASSET_BLOB_BYTES: u64 = 8 * 1024 * 1024;
const MAX_FONT_ASSET_BLOB_BYTES: u64 = 128 * 1024 * 1024;
const MAX_HUD_ASSET_BLOB_BYTES: u64 = 8 * 1024 * 1024;

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

mod actor_carrier;
mod audio_carrier;
mod audio_pcm_carrier;
pub(crate) mod equipment_carrier;
pub(crate) use actor_carrier::actor_artwork;
pub use actor_carrier::{ACTOR_ASSETS_FILENAME, actor_asset_path, require_actor_assets};
pub(crate) use audio_pcm_carrier::load_audio_pcm_assets;
pub(crate) use equipment_carrier::load_optional_equipment_assets;
mod hud_carrier;
mod icon_carrier;
mod lang_carrier;
mod path_selection;
#[cfg(test)]
pub(crate) mod test_carriers;

/// Environment override consumed through [`path_selection`]; kept beside the
/// other identity anchors so the registered marker's declared consumer stays
/// this module.
pub const ASSET_PATH_ENVIRONMENT: &str = crate::acceptance::markers::ASSETS;

pub use path_selection::{
    AssetPathSource, AssetSelection, DEFAULT_ASSET_PATH, select_asset_path,
    select_asset_path_from_environment, select_asset_path_in_context,
    select_asset_path_with_default,
};

pub use audio_carrier::{
    LoadedAudioAssets, audio_asset_path, audio_assets_missing_notice, audio_assets_rebuild_command,
    load_audio_assets,
};
pub use hud_carrier::{
    LoadedHudAssets, hud_asset_path, hud_assets_missing_notice, hud_assets_rebuild_command,
    load_hud_assets, require_hud_assets,
};
/// The embedded canonical `vanilla-source.json`, exposed so callers can bind
/// carrier provenance to the same identity startup itself validates against.
#[must_use]
pub fn vanilla_source_manifest_json() -> &'static str {
    VANILLA_SOURCE_JSON
}

pub use icon_carrier::{
    ICON_ASSETS_COMPILE_COMMAND, LoadedIconAssets, icon_asset_path, icon_assets_rebuild_command,
    require_icon_assets,
};
pub(crate) use lang_carrier::active_language;
pub use lang_carrier::{
    LANG_ASSETS_COMPILE_COMMAND, LoadedLangAssets, lang_asset_path, lang_assets_rebuild_command,
    load_active_language, require_lang_assets,
};

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
                "font asset carrier was not found at {}; using bounded diagnostic font fallback; build the reviewed Cinnangles Sans carrier with: {}",
                self.selected_path.display(),
                FONT_ASSETS_COMPILE_COMMAND
            );
        }
        format!("loaded font assets from {}", self.selected_path.display())
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
    #[error(
        "invalid local finite PCM carrier at {path}: {detail}; rebuild with make audio-pcm-assets"
    )]
    AudioPcm { path: PathBuf, detail: String },
    #[error(
        "required neutral actor carrier at {path} is unavailable or invalid: {detail}\nrebuild with: {rebuild_command}"
    )]
    ActorAssets {
        path: PathBuf,
        detail: Box<str>,
        rebuild_command: String,
    },
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
        "compiled asset carrier at {path} has stale provenance ({component}: expected SHA-256 {expected}, found {actual})\nrebuild stale local assets with: {rebuild_command}"
    )]
    WorldAssetsProvenance {
        path: PathBuf,
        component: &'static str,
        expected: String,
        actual: String,
        rebuild_command: &'static str,
    },

    #[error(
        "checkout conflict: the pinned block registry stamps wire protocol {actual} but the active content authority binds protocol {expected}; regenerate the world and physics registry inputs together"
    )]
    PinnedRegistryProtocolMismatch { expected: u32, actual: u32 },

    #[error("could not read the wire protocol stamp from the pinned block registry: {source}")]
    PinnedRegistryHeader {
        #[source]
        source: Box<AssetError>,
    },

    #[error(
        "required atmosphere asset carrier at {path} has stale provenance (expected source manifest SHA-256 {expected}, found {actual})\nrebuild local atmosphere assets with: {rebuild_command}"
    )]
    AtmosphereAssetsProvenance {
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
        rebuild_command: String,
    },

    #[error(
        "local HUD asset carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild local HUD assets with: {rebuild_command}"
    )]
    HudAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: String,
    },

    #[error(
        "could not decode local HUD asset carrier at {path}: {source}\nrebuild local HUD assets with: {rebuild_command}"
    )]
    HudAssetsDecode {
        path: PathBuf,
        #[source]
        source: HudCatalogError,
        rebuild_command: String,
    },

    #[error("{notice}")]
    HudAssetsMissing {
        path: PathBuf,
        rebuild_command: String,
        notice: String,
    },

    #[error(
        "could not read required localization carrier at {path}: {source}\nrebuild localization assets with: {rebuild_command}"
    )]
    LangAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: String,
    },

    #[error(
        "local localization carrier at {path} exceeds the {max_bytes}-byte startup limit\nrebuild localization assets with: {rebuild_command}"
    )]
    LangAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: String,
    },

    #[error(
        "could not decode local localization carrier at {path}: {source}\nrebuild localization assets with: {rebuild_command}"
    )]
    LangAssetsDecode {
        path: PathBuf,
        #[source]
        source: assets::LangCatalogError,
        rebuild_command: String,
    },

    #[error(
        "local localization carrier at {path} was compiled from manifest {carrier} but the checkout pins {manifest}\nrebuild localization assets with: {rebuild_command}"
    )]
    LangAssetsProvenance {
        path: PathBuf,
        carrier: String,
        manifest: String,
        rebuild_command: String,
    },

    #[error(
        "local localization carrier at {path} was compiled from texts/en_US.lang bytes {carrier} but the checkout pins {pinned}\nrebuild localization assets with: {rebuild_command}"
    )]
    LangAssetsSourceProvenance {
        path: PathBuf,
        carrier: String,
        pinned: String,
        rebuild_command: String,
    },

    #[error("{notice}")]
    LangAssetsMissing {
        path: PathBuf,
        rebuild_command: String,
        notice: String,
    },

    #[error(
        "could not read local sound-definition carrier at {path}: {source}
rebuild sound-definition assets with: {rebuild_command}"
    )]
    AudioAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: String,
    },

    #[error(
        "local sound-definition carrier at {path} exceeds the {max_bytes}-byte startup limit
rebuild sound-definition assets with: {rebuild_command}"
    )]
    AudioAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: String,
    },

    #[error(
        "could not decode local sound-definition carrier at {path}: {source}
rebuild sound-definition assets with: {rebuild_command}"
    )]
    AudioAssetsDecode {
        path: PathBuf,
        #[source]
        source: Box<assets::AudioCatalogError>,
        rebuild_command: String,
    },

    #[error(
        "local sound-definition carrier at {path} was compiled from manifest {carrier} but the checkout pins {manifest}
rebuild sound-definition assets with: {rebuild_command}"
    )]
    AudioAssetsProvenance {
        path: PathBuf,
        carrier: String,
        manifest: String,
        rebuild_command: String,
    },

    #[error(
        "could not read required item-icon carrier at {path}: {source}
rebuild item-icon assets with: {rebuild_command}"
    )]
    IconAssetsRead {
        path: PathBuf,
        #[source]
        source: io::Error,
        rebuild_command: String,
    },

    #[error(
        "local item-icon carrier at {path} exceeds the {max_bytes}-byte startup limit
rebuild item-icon assets with: {rebuild_command}"
    )]
    IconAssetsTooLarge {
        path: PathBuf,
        max_bytes: u64,
        rebuild_command: String,
    },

    #[error(
        "could not decode local item-icon carrier at {path}: {source}
rebuild item-icon assets with: {rebuild_command}"
    )]
    IconAssetsDecode {
        path: PathBuf,
        #[source]
        source: Box<assets::AssetError>,
        rebuild_command: String,
    },

    #[error(
        "local item-icon carrier at {path} was compiled from manifest {carrier} but the checkout pins {manifest}
rebuild item-icon assets with: {rebuild_command}"
    )]
    IconAssetsProvenance {
        path: PathBuf,
        carrier: String,
        manifest: String,
        rebuild_command: String,
    },

    #[error("{notice}")]
    IconAssetsMissing {
        path: PathBuf,
        rebuild_command: String,
        notice: String,
    },
}

#[derive(Deserialize)]
struct VanillaSource {
    tag: String,
    sha256: String,
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

pub fn load_runtime_assets(selection: AssetSelection) -> Result<LoadedAssets, AssetStartupError> {
    load_runtime_assets_timed(selection, &LoadTimes::default())
}

/// [`load_runtime_assets`], loading its four carriers in parallel and timing each into `times`.
/// Errors keep the serial order: world, atmosphere, entity, then font.
pub(crate) fn load_runtime_assets_timed(
    selection: AssetSelection,
    times: &LoadTimes,
) -> Result<LoadedAssets, AssetStartupError> {
    let source: VanillaSource = serde_json::from_str(VANILLA_SOURCE_JSON)?;
    let path = selection.path.as_path();
    let (world, atmosphere, entities, fonts) = std::thread::scope(|scope| {
        let atmosphere = scope.spawn(|| times.time("atmosphere", || load_atmosphere_assets(path)));
        let entities = scope.spawn(|| times.time("entity", || load_entity_assets(path)));
        let fonts = scope.spawn(|| times.time("font", || load_font_assets(path)));
        let world = times.time("world", || load_world_carrier(path));
        (world, join(atmosphere), join(entities), join(fonts))
    });
    let Some((runtime, blob_sha256)) = world? else {
        return Ok(diagnostic_assets(
            selection,
            source,
            atmosphere?,
            entities?,
            fonts?,
        ));
    };
    let metrics = runtime_metrics(&runtime, source, blob_sha256);
    Ok(LoadedAssets {
        runtime,
        atmosphere: atmosphere?,
        entities: entities?,
        fonts: fonts?,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::CompiledBlob,
        notice: None,
    })
}

/// The decoded world carrier and its SHA-256, or `None` when it is absent.
fn load_world_carrier(
    path: &Path,
) -> Result<Option<(Arc<RuntimeAssets>, String)>, AssetStartupError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(AssetStartupError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    if file
        .metadata()
        .map_err(|source| AssetStartupError::Read {
            path: path.to_owned(),
            source,
        })?
        .len()
        > MAX_RUNTIME_BLOB_BYTES
    {
        return Err(AssetStartupError::TooLarge {
            path: path.to_owned(),
            max_bytes: MAX_RUNTIME_BLOB_BYTES,
        });
    }

    let mut bytes = Vec::new();
    file.take(MAX_RUNTIME_BLOB_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| AssetStartupError::Read {
            path: path.to_owned(),
            source,
        })?;
    if bytes.len() as u64 > MAX_RUNTIME_BLOB_BYTES {
        return Err(AssetStartupError::TooLarge {
            path: path.to_owned(),
            max_bytes: MAX_RUNTIME_BLOB_BYTES,
        });
    }

    let (runtime, identity) =
        RuntimeAssets::decode_sealed(&bytes).map_err(|source| AssetStartupError::Decode {
            path: path.to_owned(),
            source: Box::new(source),
            rebuild_command: COMPILE_COMMAND,
        })?;
    let runtime = Arc::new(runtime);
    if let Some(keys) = load_material_keys(path, runtime.material_count()) {
        crate::runtime::network::set_base_terrain_catalog(keys.aliases());
        crate::runtime::network::set_base_material_keys(keys);
    }
    if let Some(refs) = load_vanilla_entity_refs(path) {
        crate::runtime::network::entity_pack::set_vanilla_refs(refs);
    }
    world_provenance::verify_world_carrier(path, &runtime)?;
    Ok(Some((runtime, format_sha256(identity))))
}

/// Wall time of each startup carrier load, logged as one line.
#[derive(Default)]
pub(crate) struct LoadTimes(std::sync::Mutex<Vec<(&'static str, std::time::Duration)>>);

impl LoadTimes {
    pub(crate) fn time<T>(&self, carrier: &'static str, load: impl FnOnce() -> T) -> T {
        let start = std::time::Instant::now();
        let value = load();
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((carrier, start.elapsed()));
        value
    }

    /// The carriers loaded so far, in completion order.
    #[cfg(test)]
    pub(crate) fn carriers(&self) -> Vec<&'static str> {
        let times = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        times.iter().map(|(carrier, _)| *carrier).collect()
    }

    pub(crate) fn summary(&self, wall: std::time::Duration) -> String {
        let mut times = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        times.sort_by_key(|time| std::cmp::Reverse(time.1));
        let each = times
            .iter()
            .map(|(carrier, time)| format!("{carrier} {:.1}", time.as_secs_f64() * 1e3))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "loaded startup carriers in {:.1} ms (ms each: {each})",
            wall.as_secs_f64() * 1e3
        )
    }
}

/// Joins a scoped load, re-raising its panic on this thread.
pub(crate) fn join<T>(handle: std::thread::ScopedJoinHandle<'_, T>) -> T {
    handle
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// SHA-256 of the manifest with line endings canonicalized to LF, matching
/// the compiler-side identity regardless of checkout autocrlf. One shared
/// implementation lives in the `assets` crate and serves both sides.
#[must_use]
pub fn canonical_source_manifest_sha256(source: &str) -> [u8; 32] {
    assets::canonical_source_manifest_sha256(source.as_bytes())
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
         Fetch and compile the local vanilla pack explicitly (the app never downloads it):\n  {FETCH_COMMAND}\n  {}",
        selection.path.display(),
        COMPILE_COMMAND
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
