//! Parallel runtime carrier loading and diagnostic fallback construction.
use super::{
    ATMOSPHERE_FILENAME, AssetSelection, AssetStartupError, COMPILE_COMMAND, CarrierMetrics,
    ENTITY_ASSETS_FILENAME, FETCH_COMMAND, FONT_ASSETS_FILENAME, LOCAL_FONT_ASSETS_FILENAME,
    LoadedAssetKind, LoadedAssets, LoadedAtmosphereAssets, LoadedEntityAssets, LoadedFontAssets,
    MAX_RUNTIME_BLOB_BYTES, VANILLA_SOURCE_JSON, format_sha256,
    optional_carriers::{
        load_atmosphere_assets, load_entity_assets, load_font_assets, load_material_keys,
        load_vanilla_entity_refs,
    },
    world_provenance,
};
use crate::RuntimeAssets;
use serde::Deserialize;
use std::path::PathBuf;
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
    sync::Arc,
};

#[derive(Deserialize)]
struct VanillaSource {
    tag: String,
    sha256: String,
}

/// Selects the atmosphere carrier beside the world carrier.
#[must_use]
pub fn atmosphere_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(ATMOSPHERE_FILENAME)
}

/// Selects the entity carrier beside the world carrier.
#[must_use]
pub fn entity_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(ENTITY_ASSETS_FILENAME)
}

/// Selects the bundled font carrier beside the world carrier.
#[must_use]
pub fn font_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(FONT_ASSETS_FILENAME)
}

/// Selects the optional local font carrier beside the world carrier.
#[must_use]
pub fn local_font_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(LOCAL_FONT_ASSETS_FILENAME)
}

/// Loads and validates the runtime carriers without recording wall-clock timings.
pub fn load_runtime_assets(selection: AssetSelection) -> Result<LoadedAssets, AssetStartupError> {
    load_runtime_assets_observed(selection, &())
}

/// [`load_runtime_assets`], loading its four carriers in parallel through `observer`.
/// Errors keep the serial order: world, atmosphere, entity, then font.
pub fn load_runtime_assets_observed(
    selection: AssetSelection,
    observer: &impl CarrierLoadObserver,
) -> Result<LoadedAssets, AssetStartupError> {
    let source: VanillaSource = serde_json::from_str(VANILLA_SOURCE_JSON)?;
    let path = selection.path.as_path();
    let (world, atmosphere, entities, fonts) = std::thread::scope(|scope| {
        let atmosphere =
            scope.spawn(|| observer.observe("atmosphere", || load_atmosphere_assets(path)));
        let entities = scope.spawn(|| observer.observe("entity", || load_entity_assets(path)));
        let fonts = scope.spawn(|| observer.observe("font", || load_font_assets(path)));
        let world = observer.observe("world", || load_world_carrier(path));
        (world, join(atmosphere), join(entities), join(fonts))
    });
    let Some(world) = world? else {
        return Ok(diagnostic_assets(
            selection,
            source,
            atmosphere?,
            entities?,
            fonts?,
        ));
    };
    let metrics = runtime_metrics(&world.runtime, source, world.blob_sha256);
    Ok(LoadedAssets {
        runtime: world.runtime,
        material_keys: world.material_keys,
        vanilla_entity_refs: world.vanilla_entity_refs,
        atmosphere: atmosphere?,
        entities: entities?,
        fonts: fonts?,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::CompiledBlob,
        notice: None,
    })
}

/// The decoded world carrier, identity and optional pack sidecars.
struct LoadedWorldAssets {
    runtime: Arc<RuntimeAssets>,
    blob_sha256: String,
    material_keys: Option<crate::MaterialKeys>,
    vanilla_entity_refs: Option<crate::VanillaEntityRefs>,
}

/// The decoded world carrier and its SHA-256, or `None` when it is absent.
fn load_world_carrier(path: &Path) -> Result<Option<LoadedWorldAssets>, AssetStartupError> {
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
    let material_keys = load_material_keys(path, runtime.material_count());
    let vanilla_entity_refs = load_vanilla_entity_refs(path);
    world_provenance::verify_world_carrier(path, &runtime)?;
    Ok(Some(LoadedWorldAssets {
        runtime,
        blob_sha256: format_sha256(identity),
        material_keys,
        vanilla_entity_refs,
    }))
}

/// Observes a carrier load without making the asset loader depend on a clock.
pub trait CarrierLoadObserver: Sync {
    /// Runs the load and returns its result, allowing the caller to record its duration.
    fn observe<T>(&self, carrier: &'static str, load: impl FnOnce() -> T) -> T;
}

impl CarrierLoadObserver for () {
    /// Runs the carrier load without instrumentation.
    fn observe<T>(&self, _carrier: &'static str, load: impl FnOnce() -> T) -> T {
        load()
    }
}

/// Joins a scoped load, re-raising its panic on this thread.
fn join<T>(handle: std::thread::ScopedJoinHandle<'_, T>) -> T {
    handle
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Supplies diagnostic terrain while preserving the validated sibling carriers.
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
        material_keys: None,
        vanilla_entity_refs: None,
        atmosphere,
        entities,
        fonts,
        metrics,
        selected_path: selection.path,
        kind: LoadedAssetKind::Diagnostic,
        notice: Some(notice),
    }
}

/// Counts the decoded terrain data for startup and runtime diagnostics.
fn runtime_metrics(
    runtime: &RuntimeAssets,
    source: VanillaSource,
    blob_sha256: String,
) -> CarrierMetrics {
    let pages = runtime.texture_pages();
    CarrierMetrics {
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
    }
}
