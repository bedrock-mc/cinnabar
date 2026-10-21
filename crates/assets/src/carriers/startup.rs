//! Startup carrier selection, validation and decoded data for runtime consumers.
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{RuntimeAssets, RuntimeAtmosphereAssets, RuntimeEntityAssets, RuntimeFontCatalog};

mod font_fallback;
pub use font_fallback::diagnostic_font_catalog;
mod optional_carriers;
pub(crate) use optional_carriers::shell_quote_path;
mod error;
pub use error::AssetStartupError;
mod entity_facts;
mod loading;
pub use entity_facts::require_entity_facts;
pub use loading::{
    CarrierLoadObserver, atmosphere_asset_path, entity_asset_path, font_asset_path,
    load_runtime_assets, load_runtime_assets_observed, local_font_asset_path,
};
pub mod delivery;
mod world_provenance;

pub const ATMOSPHERE_FILENAME: &str = crate::carriers::ATMOSPHERE.output;
pub const ATMOSPHERE_COMPILE_COMMAND: &str = "make atmosphere-assets";
pub const ENTITY_ASSETS_FILENAME: &str = crate::carriers::ENTITY.output;
pub const ENTITY_ASSETS_COMPILE_COMMAND: &str = "make entity-assets";
pub const FONT_ASSETS_FILENAME: &str = crate::carriers::FONT.output;
pub const FONT_ASSETS_COMPILE_COMMAND: &str = "make font-assets";
pub const LOCAL_FONT_ASSETS_FILENAME: &str = "vanilla-v1.mcbefont";
pub const LOCAL_FONT_ASSETS_COMPILE_COMMAND: &str =
    "make font-assets-local FONT_PACK_DIR=<reviewed-font-pack>";
pub const HUD_ASSETS_FILENAME: &str = crate::carriers::HUD.output;
pub const HUD_ASSETS_REPORT_FILENAME: &str = crate::carriers::HUD.report.unwrap();
pub const HUD_ASSETS_COMPILE_COMMAND: &str = "make hud-assets";
pub const AUDIO_ASSETS_FILENAME: &str = crate::carriers::AUDIO.output;
pub const AUDIO_ASSETS_COMPILE_COMMAND: &str = "make audio-assets";
pub const FETCH_COMMAND: &str = "make vanilla-assets";
pub const COMPILE_COMMAND: &str = "make world-assets";

const VANILLA_SOURCE_JSON: &str = crate::VANILLA_SOURCE_MANIFEST;
const FONT_SOURCE_JSON: &str = include_str!("../../../../assets/cinnangles-sans-source.json");
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierMetrics {
    pub source_tag: String,
    pub source_sha256: String,
    pub blob_sha256: String,
    pub texture_layers: u32,
    pub texture_pages: u32,
    pub texture_bytes_including_mips: u64,
    pub material_count: u32,
    pub model_template_count: u32,
    pub model_quad_count: u32,
    pub animation_count: u32,
    pub animation_frame_count: u32,
    pub missing_mapping_count: u64,
}

pub struct LoadedAssets {
    pub runtime: Arc<RuntimeAssets>,
    pub item_facts: Arc<crate::facts::ItemFacts>,
    pub atmosphere: LoadedAtmosphereAssets,
    pub entities: LoadedEntityAssets,
    pub fonts: LoadedFontAssets,
    pub metrics: CarrierMetrics,
    pub material_keys: Option<crate::material_keys::MaterialKeys>,
    pub vanilla_entity_refs: Option<crate::VanillaEntityRefs>,
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
mod equipment_carrier;
pub use actor_carrier::{ACTOR_ASSETS_FILENAME, actor_asset_path, require_actor_assets};
pub use audio_pcm_carrier::load_audio_pcm_assets;
pub use equipment_carrier::{equipment_asset_path, load_optional_equipment_assets};
mod item_facts_carrier;
pub use item_facts_carrier::require_item_facts;
mod hud_carrier;
mod icon_carrier;
mod lang_carrier;
pub mod markers;
mod path_selection;
#[cfg(any(test, feature = "test-support"))]
pub mod test_carriers;

/// Environment override used when no command-line carrier path was supplied.
pub const ASSET_PATH_ENVIRONMENT: &str = markers::ASSETS;

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
pub use lang_carrier::{
    LANG_ASSETS_COMPILE_COMMAND, LoadedLangAssets, lang_asset_path, lang_assets_rebuild_command,
    require_lang_assets,
};
pub use lang_carrier::{active_language, load_optional_language};

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

impl LoadedAtmosphereAssets {
    /// The path from which this carrier was loaded.
    #[must_use]
    pub fn selected_path(&self) -> &Path {
        &self.selected_path
    }

    /// The validated carrier envelope identity.
    #[must_use]
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }

    /// Transfers the decoded carrier and its envelope identity to the renderer.
    pub fn into_parts(self) -> (Arc<RuntimeAtmosphereAssets>, [u8; 32]) {
        (self.runtime, self.identity)
    }
}

/// Formats a carrier identity as lowercase hexadecimal.
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

/// SHA-256 of the manifest with line endings canonicalized to LF, matching
/// the compiler-side identity regardless of checkout autocrlf. One shared
/// implementation lives in the `assets` crate and serves both sides.
#[must_use]
pub fn canonical_source_manifest_sha256(source: &str) -> [u8; 32] {
    crate::canonical_source_manifest_sha256(source.as_bytes())
}
